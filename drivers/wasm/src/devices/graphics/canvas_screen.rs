use core::slice;

use alloc::{boxed::Box, rc::Rc, string::String, vec::Vec};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::{Area, GET_RESOLUTION, Point, RenderingColor, SET_DRAWING_AREA, WAS_RESIZED};
use shared::{AnyByLayout, align_slice_to};
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};
use task::block_on;
use wasm_bindgen::{Clamped, JsCast, JsValue, prelude::Closure};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use crate::devices::graphics::get_window_size;

struct Inner {
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    conversion_buffer: Vec<u8>,
    area: Area,
    was_resized: bool,
}

impl Inner {
    fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"alpha".into(), &JsValue::FALSE).unwrap();

        let context = canvas
            .get_context_with_context_options("2d", &options)
            .unwrap()
            .ok_or("No 2D context found")?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "Failed to cast to CanvasRenderingContext2d")?;

        let canvas_screen = Self {
            canvas,
            context,
            conversion_buffer: Vec::new(),
            area: Area::default(),
            was_resized: false,
        };

        Ok(canvas_screen)
    }

    fn get_resolution(&self) -> Result<Point, String> {
        Ok(Point::new(
            self.canvas.width() as _,
            self.canvas.height() as _,
        ))
    }

    #[target_feature(enable = "simd128")]
    unsafe fn rgba_to_bgra_simd(rgba: &[u8], bgra: &mut [u8]) {
        use core::arch::wasm32::*;

        assert_eq!(rgba.len(), bgra.len());
        assert_eq!(rgba.len() % 4, 0, "pixel data must be RGBA-sized");
        let len = rgba.len();
        let mut i = 0;
        while i + 16 <= len {
            let v = unsafe { v128_load(rgba.as_ptr().add(i) as *const v128) };
            // shuffle mask: swap R and B (0 <-> 2)
            let swapped =
                i8x16_shuffle::<2, 1, 0, 3, 6, 5, 4, 7, 10, 9, 8, 11, 14, 13, 12, 15>(v, v);
            unsafe {
                v128_store(bgra.as_mut_ptr().add(i) as *mut v128, swapped);
            }
            i += 16;
        }

        // XRGB8888 on little-endian WASM stores pixels as B,G,R,X.
        // Canvas ImageData expects R,G,B,A. Swap the remaining complete pixels.
        for (src, dst) in rgba[i..].chunks_exact(4).zip(bgra[i..].chunks_exact_mut(4)) {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
    }

    fn draw_buffer(&mut self, buffer: &[RenderingColor]) -> Result<(), String> {
        let pixel_count = self.area.get_width() as usize * self.area.get_height() as usize;
        let bytes_needed = pixel_count * size_of::<RenderingColor>();

        if self.conversion_buffer.capacity() < bytes_needed {
            self.conversion_buffer
                .reserve(bytes_needed - self.conversion_buffer.len());
        }
        unsafe {
            self.conversion_buffer.set_len(bytes_needed);
        }

        let destination =
            unsafe { slice::from_raw_parts_mut(self.conversion_buffer.as_mut_ptr(), bytes_needed) };

        let source = unsafe { slice::from_raw_parts(buffer.as_ptr() as *const u8, bytes_needed) };

        unsafe {
            Self::rgba_to_bgra_simd(source, destination);
        }

        // now conversion_buffer is ready as &[u8]
        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&self.conversion_buffer),
            self.area.get_width() as _,
            self.area.get_height() as _,
        )
        .expect("Failed to create ImageData");

        let (x, y) = self.area.get_point_1().into();
        self.context
            .put_image_data(&image_data, x as _, y as _)
            .unwrap();

        Ok(())
    }
}

pub struct CanvasScreenDevice(Rc<RwLock<CriticalSectionRawMutex, Inner>>);

unsafe impl Sync for CanvasScreenDevice {}
unsafe impl Send for CanvasScreenDevice {}

impl CanvasScreenDevice {
    pub async fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let device = Self(Rc::new(RwLock::new(Inner::new(canvas.clone())?)));

        let window = web_sys::window().ok_or("Failed to get window")?;
        let device_clone = device.0.clone();
        let window_clone = window.clone();
        let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if let Some((width, height)) = get_window_size(&window_clone) {
                // Setting either dimension clears the bitmap even when its
                // value is unchanged. LVGL does not redraw for an unchanged
                // resolution, so ignore redundant browser resize events.
                if canvas.width() != width || canvas.height() != height {
                    canvas.set_width(width);
                    canvas.set_height(height);
                    block_on(device_clone.write()).was_resized = true;
                }
            } else {
                log::error!("Failed to get window size");
            }
        }) as Box<dyn FnMut(_)>);

        window
            .add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref())
            .unwrap();

        closure.forget(); // Prevent memory corruption by keeping the closure alive

        Ok(device)
    }
}

impl DirectBaseOperations for CanvasScreenDevice {
    fn read(&self, _: &mut [u8], _: Size) -> file_system::Result<usize> {
        Err(file_system::Error::UnsupportedOperation)
    }

    fn write(&self, buffer: &[u8], _: Size) -> file_system::Result<usize> {
        let buffer: &[RenderingColor] =
            align_slice_to(buffer).ok_or(file_system::Error::InvalidParameter)?;

        let mut inner = block_on(self.0.write());

        inner
            .draw_buffer(buffer)
            .map_err(|_| file_system::Error::InputOutput)?;

        Ok(buffer.len())
    }

    fn control(
        &self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> file_system::Result<()> {
        match command {
            SET_DRAWING_AREA::IDENTIFIER => {
                let area = SET_DRAWING_AREA::cast_input(input)?;

                let mut inner = block_on(self.0.write());
                inner.area = *area;
            }
            GET_RESOLUTION::IDENTIFIER => {
                let point = GET_RESOLUTION::cast_output(output)?;

                let inner = block_on(self.0.read());

                let resolution = inner
                    .get_resolution()
                    .map_err(|_| file_system::Error::InputOutput)?;

                *point = resolution;
            }
            WAS_RESIZED::IDENTIFIER => {
                let was_resized = WAS_RESIZED::cast_output(output)?;

                let mut inner = block_on(self.0.write());
                *was_resized = inner.was_resized;
                inner.was_resized = false;
            }
            _ => return Err(file_system::Error::InvalidParameter),
        }

        Ok(())
    }
}

impl MountOperations for CanvasScreenDevice {}

impl DirectCharacterDevice for CanvasScreenDevice {}

#[cfg(test)]
mod color_conversion_tests {
    use alloc::{vec, vec::Vec};
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    wasm_bindgen_test_configure!(run_in_browser);

    fn scalar_swap(source: &[u8]) -> Vec<u8> {
        source
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
            .collect()
    }

    #[wasm_bindgen_test]
    fn simd_matches_scalar_for_short_and_unaligned_rectangles() {
        for pixels in 0..=17 {
            let source: Vec<u8> = (0..pixels * 4)
                .map(|index| (index * 37 + 11) as u8)
                .collect();
            let mut destination = vec![0; source.len()];
            unsafe { super::Inner::rgba_to_bgra_simd(&source, &mut destination) };
            assert_eq!(destination, scalar_swap(&source), "pixels={pixels}");
        }
    }

    #[wasm_bindgen_test]
    fn preserves_green_and_alpha_while_swapping_red_and_blue() {
        let source = [10, 20, 30, 40, 255, 128, 0, 77, 0, 255, 255, 255];
        let mut destination = [0; 12];
        unsafe { super::Inner::rgba_to_bgra_simd(&source, &mut destination) };
        assert_eq!(
            destination,
            [30, 20, 10, 40, 0, 128, 255, 77, 255, 255, 0, 255]
        );
    }
}
