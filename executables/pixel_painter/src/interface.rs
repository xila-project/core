use crate::storage;
use embedded_i18n::translate;
use pixel_painter::drawing::{Drawing, HEIGHT, PALETTE, Viewport, WIDTH};
use std::{
    ffi::{CStr, CString},
    ptr::null_mut,
    thread::sleep,
    time::Duration,
};
use wasm::*;

struct Interface {
    window: *mut Object,
    canvas: *mut Object,
    palette: *mut Object,
    color_label: *mut Object,
    actions: *mut Object,
    path: *mut Object,
    keyboard: *mut Object,
    status: *mut Object,
    drawing: Drawing,
    color: u8,
    brush: u8,
    revision: u64,
    confirm_open: Option<u64>,
    confirm_close: Option<u64>,
    // Keep static widget maps alive for LVGL's lifetime.
    _action_map: [*const i8; 9],
}

pub fn run() -> wasm::Result<()> {
    let mut ui = unsafe { Interface::new()? };
    loop {
        let mut repaint = false;
        for _ in 0..32 {
            let mut code = 0u32;
            let mut target: *mut Object = null_mut();
            let (mut x, mut y, mut width, mut height) = (0, 0, 0, 0);
            unsafe {
                window_pop_pointer_event(
                    ui.window,
                    &mut code,
                    (&mut target as *mut *mut Object).cast(),
                    &mut x,
                    &mut y,
                    &mut width,
                    &mut height,
                )?;
            }
            if code == EventCode::All as u32 {
                break;
            }
            if code == EVENT_CLOSE_REQUESTED {
                if !ui.drawing.is_dirty() || ui.confirm_close == Some(ui.revision) {
                    unsafe {
                        object_delete(u16::try_from(ui.window as usize).map_err(|_| -1)?)?;
                    }
                    return Ok(());
                }
                ui.status(translate!("Unsaved drawing - close again to discard"))?;
                ui.confirm_close = Some(ui.revision);
            } else if target == ui.canvas {
                if code == EventCode::Pressed as u32 || code == EventCode::Pressing as u32 {
                    if let Some((x, y)) = Viewport::new(0, 0, width, height).map(x, y) {
                        if code == EventCode::Pressed as u32 {
                            ui.drawing.begin_stroke(x, y, ui.color, ui.brush);
                        } else {
                            ui.drawing.continue_stroke(x, y, ui.color, ui.brush);
                        }
                        ui.revision = ui.revision.wrapping_add(1);
                        repaint = true;
                    } else {
                        ui.drawing.end_stroke();
                    }
                } else if code == EventCode::Released as u32 || code == EventCode::PressLost as u32
                {
                    ui.drawing.end_stroke();
                }
            } else if code == EventCode::Clicked as u32 && target == ui.palette {
                if width > 0 && (0..width).contains(&x) {
                    ui.color = ((i64::from(x) * PALETTE.len() as i64) / i64::from(width))
                        .min(PALETTE.len() as i64 - 1) as u8;
                    ui.update_color_label()?;
                }
            } else if code == EventCode::Clicked as u32 && target == ui.actions {
                let action = unsafe { buttonmatrix_get_selected_button(ui.actions)? };
                ui.drawing.end_stroke();
                match action {
                    0 => {
                        ui.drawing.undo();
                        ui.revision = ui.revision.wrapping_add(1);
                        repaint = true;
                    }
                    1 => {
                        ui.drawing.clear();
                        ui.revision = ui.revision.wrapping_add(1);
                        repaint = true;
                    }
                    2 | 3 => {
                        repaint |= ui.file_action(action == 3)?;
                    }
                    4 => ui.brush = 1,
                    5 => ui.brush = 3,
                    6 => ui.brush = 5,
                    _ => (),
                }
                if (4..=6).contains(&action) {
                    ui.update_color_label()?;
                }
            } else if code == EventCode::Focused as u32 && target == ui.path {
                unsafe {
                    keyboard_set_textarea(ui.keyboard, ui.path)?;
                    object_remove_flag(ui.keyboard, ObjectFlag::Hidden)?;
                }
            } else if (code == EventCode::Ready as u32 || code == EventCode::Cancel as u32)
                && target == ui.keyboard
            {
                unsafe {
                    object_add_flag(ui.keyboard, ObjectFlag::Hidden)?;
                }
            }
        }
        if repaint {
            ui.upload()?;
        }
        sleep(Duration::from_millis(16));
    }
}

impl Interface {
    unsafe fn new() -> wasm::Result<Self> {
        unsafe {
            let window = window_create()?;
            window_set_icon(
                window,
                c"Pa".as_ptr(),
                Color {
                    red: 142,
                    green: 36,
                    blue: 170,
                },
            )?;
            object_set_flex_flow(window, FlexFlow::Column)?;
            object_set_padding(window, 4)?;
            let color_label = label_create(window)?;
            object_set_width(color_label, percentage(100)?)?;
            object_set_text_size(color_label, 16)?;
            label_set_text(color_label, c"#202124  ·  1 px".as_ptr())?;
            let palette = indexed_canvas_create(window, PALETTE.len() as u16, 1)?;
            object_set_width(palette, percentage(100)?)?;
            object_set_height(palette, 28)?;
            object_add_flag(palette, ObjectFlag::Clickable)?;
            let swatches = [0u8, 1, 2, 3, 4, 5, 6, 7];
            indexed_canvas_set_pixels(
                palette,
                swatches.as_ptr(),
                swatches.len(),
                PALETTE.as_ptr(),
                PALETTE.len(),
            )?;
            let actions = buttonmatrix_create(window)?;
            object_set_width(actions, percentage(100)?)?;
            object_set_height(actions, 58)?;
            let action_map = [
                translate!(c"Undo").as_ptr(),
                translate!(c"Clear").as_ptr(),
                translate!(c"Save").as_ptr(),
                translate!(c"Open").as_ptr(),
                c"\n".as_ptr(),
                c"1 px".as_ptr(),
                c"3 px".as_ptr(),
                c"5 px".as_ptr(),
                c"".as_ptr(),
            ];
            buttonmatrix_set_map(actions, action_map.as_ptr())?;
            let path = textarea_create(window)?;
            textarea_set_one_line(path, true)?;
            textarea_set_max_length(path, 240)?;
            let initial = std::env::args()
                .nth(1)
                .or_else(storage::default_path)
                .unwrap_or_default();
            let initial = CString::new(initial).map_err(|_| -1)?;
            textarea_set_text(path, initial.as_ptr())?;
            object_set_width(path, percentage(100)?)?;
            object_set_height(path, 36)?;
            let canvas = indexed_canvas_create(window, WIDTH as u16, HEIGHT as u16)?;
            object_set_width(canvas, percentage(100)?)?;
            object_set_flex_grow(canvas, 1)?;
            object_add_flag(canvas, ObjectFlag::Clickable)?;
            let status = label_create(window)?;
            object_set_width(status, percentage(100)?)?;
            let keyboard = keyboard_create(window)?;
            object_set_height(keyboard, 110)?;
            object_add_flag(keyboard, ObjectFlag::Hidden)?;
            for object in [palette, actions, path, canvas, keyboard] {
                object_add_flag(object, ObjectFlag::EventBubble)?;
            }
            let ui = Self {
                window,
                canvas,
                palette,
                color_label,
                actions,
                path,
                keyboard,
                status,
                drawing: Drawing::new(),
                color: 1,
                brush: 1,
                revision: 0,
                confirm_open: None,
                confirm_close: None,
                _action_map: action_map,
            };
            ui.update_color_label()?;
            ui.status(translate!("Ready - white erases"))?;
            ui.upload()?;
            Ok(ui)
        }
    }

    fn status(&self, text: &str) -> wasm::Result<()> {
        let text = CString::new(text).map_err(|_| -1)?;
        unsafe { label_set_text(self.status, text.as_ptr().cast_mut()) }
    }

    fn update_color_label(&self) -> wasm::Result<()> {
        let text = CString::new(format!(
            "#{:06X}  ·  {} px",
            PALETTE[self.color as usize], self.brush
        ))
        .map_err(|_| -1)?;
        unsafe { label_set_text(self.color_label, text.as_ptr()) }
    }

    fn upload(&self) -> wasm::Result<()> {
        unsafe {
            indexed_canvas_set_pixels(
                self.canvas,
                self.drawing.pixels().as_ptr(),
                self.drawing.pixels().len(),
                PALETTE.as_ptr(),
                PALETTE.len(),
            )
        }
    }

    fn file_action(&mut self, opening: bool) -> wasm::Result<bool> {
        if opening && self.drawing.is_dirty() && self.confirm_open != Some(self.revision) {
            self.confirm_open = Some(self.revision);
            self.status(translate!("Unsaved drawing - Open again to discard"))?;
            return Ok(false);
        }
        self.confirm_open = None;
        // LVGL limits characters, not UTF-8 bytes: allow four bytes per character.
        let mut bytes = [0i8; 240 * 4 + 1];
        unsafe {
            textarea_get_text(self.path, bytes.as_mut_ptr(), bytes.len())?;
        }
        let path = unsafe { CStr::from_ptr(bytes.as_ptr()) }
            .to_str()
            .unwrap_or("");
        if !path.starts_with('/') || path.ends_with('/') {
            self.status(translate!("Enter an absolute file path"))?;
            return Ok(false);
        }
        if opening {
            match storage::read(path) {
                Ok(bytes) if self.drawing.replace_from_bytes(&bytes).is_ok() => {
                    self.revision = self.revision.wrapping_add(1);
                    self.status(translate!("Opened"))?;
                    return Ok(true);
                }
                Ok(_) => self.status(translate!("Invalid Pixel Painter file - drawing kept"))?,
                Err(_) => self.status(translate!("Open failed - check path and storage"))?,
            }
        } else if storage::write(path, &self.drawing.encode()).is_ok() {
            self.drawing.mark_saved();
            self.status(translate!("Saved"))?;
        } else {
            self.status(translate!(
                "Save failed - drawing kept; check path and storage"
            ))?;
        }
        Ok(false)
    }
}
