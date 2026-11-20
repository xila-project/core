//! Bounded, host-owned indexed canvas for guests. LVGL never retains guest memory.
use super::error::{Error, Result};
use crate::lvgl;
use alloc::{vec, vec::Vec};
use core::mem::size_of;

pub(super) const MAX_CANVAS_PIXELS: usize = 65_536;

#[repr(C)]
struct IndexedCanvasHeader {
    palette: [u32; 256],
    width: usize,
    height: usize,
    length: usize,
}

/// One aligned allocation: header followed by pixel bytes.
/// Moving the `Vec` owner never moves or retags its allocation.
pub(crate) struct IndexedCanvas {
    storage: Vec<usize>,
}

impl IndexedCanvas {
    fn header(&self) -> &IndexedCanvasHeader {
        // SAFETY: `new` initializes an `IndexedCanvasHeader` at the start of `storage`.
        unsafe { &*self.storage.as_ptr().cast::<IndexedCanvasHeader>() }
    }

    pub fn len(&self) -> usize {
        self.header().length
    }

    #[cfg(test)]
    pub fn pixels(&self) -> &[u8] {
        let header = self.header();
        // SAFETY: `new` allocates `size_of::<IndexedCanvasHeader>() + header.length` bytes.
        unsafe {
            core::slice::from_raw_parts(
                self.storage
                    .as_ptr()
                    .cast::<u8>()
                    .add(size_of::<IndexedCanvasHeader>()),
                header.length,
            )
        }
    }

    #[cfg(test)]
    pub fn palette(&self) -> &[u32; 256] {
        &self.header().palette
    }

    pub fn upload(
        &mut self,
        pixels: &[u8],
        palette: impl ExactSizeIterator<Item = u32>,
    ) -> Result<()> {
        let colors = palette.len();
        let length = self.len();
        if colors == 0
            || colors > 256
            || pixels.len() != length
            || pixels.iter().any(|&pixel| pixel as usize >= colors)
        {
            return Err(Error::InvalidValue);
        }
        let mut decoded = [0u32; 256];
        for (index, color) in palette.enumerate() {
            if color > 0xffffff {
                return Err(Error::InvalidValue);
            }
            decoded[index] = color;
        }
        let base = self.storage.as_mut_ptr();
        // SAFETY: `base` points to one allocation holding `IndexedCanvasHeader` followed by
        // `length` pixel bytes; both slices are disjoint and initialized.
        unsafe {
            let dest_pixels = core::slice::from_raw_parts_mut(
                base.cast::<u8>().add(size_of::<IndexedCanvasHeader>()),
                length,
            );
            dest_pixels.copy_from_slice(pixels);
            (*base.cast::<IndexedCanvasHeader>()).palette = decoded;
        }
        Ok(())
    }

    pub fn new(width: usize, height: usize) -> Result<Self> {
        let length = width.checked_mul(height).ok_or(Error::InvalidValue)?;
        if width == 0 || height == 0 || length > MAX_CANVAS_PIXELS {
            return Err(Error::InvalidValue);
        }
        let total_bytes = size_of::<IndexedCanvasHeader>() + length;
        let words = total_bytes.div_ceil(size_of::<usize>());
        let mut storage = vec![0usize; words];
        // SAFETY: `storage` has at least `size_of::<IndexedCanvasHeader>()` bytes and
        // `usize` alignment (`align_of::<IndexedCanvasHeader>() == align_of::<usize>()`).
        unsafe {
            storage
                .as_mut_ptr()
                .cast::<IndexedCanvasHeader>()
                .write(IndexedCanvasHeader {
                    palette: [0xffffff; 256],
                    width,
                    height,
                    length,
                });
        }
        Ok(Self { storage })
    }

    pub unsafe fn attach(&mut self, object: *mut lvgl::lv_obj_t) -> Result<()> {
        unsafe {
            lvgl::lv_obj_set_style_pad_all(object, 0, 0);
            lvgl::lv_obj_set_style_border_width(object, 0, 0);
            lvgl::lv_obj_remove_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE);
            let callback = lvgl::lv_obj_add_event_cb(
                object,
                Some(draw),
                lvgl::lv_event_code_t_LV_EVENT_DRAW_MAIN,
                self.storage.as_mut_ptr().cast(),
            );
            if callback.is_null() {
                return Err(Error::NativeFailure);
            }
        }
        Ok(())
    }
}

unsafe extern "C" fn draw(event: *mut lvgl::lv_event_t) {
    unsafe {
        let header = &*lvgl::lv_event_get_user_data(event).cast::<IndexedCanvasHeader>();
        let pixels = core::slice::from_raw_parts(
            (header as *const IndexedCanvasHeader).add(1).cast::<u8>(),
            header.length,
        );
        let object = lvgl::lv_event_get_target(event).cast::<lvgl::lv_obj_t>();
        let mut bounds = lvgl::lv_area_t::default();
        lvgl::lv_obj_get_content_coords(object, &mut bounds);
        let available_width = (bounds.x2 - bounds.x1 + 1).max(0);
        let available_height = (bounds.y2 - bounds.y1 + 1).max(0);
        let width = i64::from(available_width)
            .min(i64::from(available_height) * header.width as i64 / header.height as i64)
            as i32;
        let height = i64::from(available_height)
            .min(i64::from(available_width) * header.height as i64 / header.width as i64)
            as i32;
        let origin_x = bounds.x1 + (available_width - width) / 2;
        let origin_y = bounds.y1 + (available_height - height) / 2;
        let layer = lvgl::lv_event_get_layer(event);
        let mut descriptor = lvgl::lv_draw_rect_dsc_t::default();
        lvgl::lv_draw_rect_dsc_init(&mut descriptor);
        descriptor.bg_opa = lvgl::LV_OPA_COVER;
        // Coalesce equal-color runs without creating an LVGL object per pixel.
        for y in 0..header.height {
            let row = &pixels[y * header.width..(y + 1) * header.width];
            let mut x = 0;
            while x < header.width {
                let start = x;
                let color = row[x];
                while x < header.width && row[x] == color {
                    x += 1;
                }
                let area = lvgl::lv_area_t {
                    x1: origin_x + scale(start, width, header.width),
                    x2: origin_x + scale(x, width, header.width) - 1,
                    y1: origin_y + scale(y, height, header.height),
                    y2: origin_y + scale(y + 1, height, header.height) - 1,
                };
                if area.x1 <= area.x2 && area.y1 <= area.y2 {
                    descriptor.bg_color = lvgl::lv_color_hex(header.palette[color as usize]);
                    lvgl::lv_draw_rect(layer, &descriptor, &area);
                }
            }
        }
    }
}

// LVGL dimensions can be large even when the retained image is small.
fn scale(index: usize, length: i32, total: usize) -> i32 {
    // Ceil the boundary to match floor-based pointer mapping, including downscaling.
    ((index as i64 * i64::from(length) + total as i64 - 1) / total as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_boundaries_match_pointer_mapping_at_fractional_scales() {
        for width in [80, 217, 480] {
            for screen_x in 0..width {
                let pixel = screen_x * 160 / width;
                assert!(scale(pixel as usize, width, 160) <= screen_x);
                assert!(screen_x < scale(pixel as usize + 1, width, 160));
            }
        }
    }

    #[test]
    fn uploads_copy_input_and_reject_invalid_images_without_mutation() {
        let mut canvas = IndexedCanvas::new(2, 2).unwrap();
        let mut pixels = [0, 1, 1, 0];
        canvas
            .upload(&pixels, [0xffffff, 0xff0000].into_iter())
            .unwrap();
        pixels.fill(255);
        assert_eq!(canvas.pixels(), [0, 1, 1, 0]);
        assert_eq!(canvas.palette()[1], 0xff0000);
        for (pixels, palette) in [
            (&[0u8, 1, 1][..], vec![0, 1]),
            (&[0, 2, 1, 0][..], vec![0, 1]),
            (&[0, 1, 1, 0][..], vec![0, 0x1000000]),
        ] {
            assert_eq!(
                canvas.upload(pixels, palette.into_iter()),
                Err(Error::InvalidValue)
            );
            assert_eq!(canvas.pixels(), [0, 1, 1, 0]);
            assert_eq!(canvas.palette()[1], 0xff0000);
        }
        assert!(IndexedCanvas::new(0, 2).is_err());
        assert!(IndexedCanvas::new(300, 300).is_err());
    }
}
