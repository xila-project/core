use crate::storage::{self, PAGE_BYTES, Page, WorkingCopy};
use embedded_i18n::translate;
use std::{
    ffi::{CStr, CString},
    path::Path,
    ptr::null_mut,
    thread::sleep,
    time::Duration,
};
use wasm::*;

struct Interface {
    window: *mut Object,
    actions: *mut Object,
    path: *mut Object,
    text: *mut Object,
    keyboard: *mut Object,
    status: *mut Object,
    page_label: *mut Object,
    working: Option<WorkingCopy>,
    original_page: String,
    page_start: u64,
    page_length: usize,
    dirty: bool,
    revision: u64,
    confirm_open: Option<u64>,
    confirm_close: Option<u64>,
    working_unusable: bool,
    _action_map: [*const i8; 7],
}

pub fn run() -> wasm::Result<()> {
    let mut ui = unsafe { Interface::new()? };
    loop {
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
                if ui.working_unusable {
                    if ui.confirm_close != Some(ui.revision) {
                        ui.status(translate!("errors.edit_failed_close_again"))?;
                        ui.confirm_close = Some(ui.revision);
                        continue;
                    }
                } else if !ui.commit_page()? {
                    continue;
                }
                if !ui.dirty || ui.confirm_close == Some(ui.revision) {
                    unsafe {
                        object_delete(u16::try_from(ui.window as usize).map_err(|_| -1)?)?;
                    }
                    return Ok(());
                }
                ui.status(translate!("warnings.unsaved_close_again"))?;
                ui.confirm_close = Some(ui.revision);
            } else if code == EventCode::ValueChanged as u32 && target == ui.actions {
                let action = unsafe { buttonmatrix_get_selected_button(ui.actions)? };
                match action {
                    0 => ui.navigate(false)?,
                    1 => ui.navigate(true)?,
                    2 => ui.new_document()?,
                    3 => ui.open_document()?,
                    4 => ui.save_document()?,
                    _ => (),
                }
            } else if code == EventCode::Focused as u32 && (target == ui.path || target == ui.text)
            {
                unsafe {
                    keyboard_set_textarea(ui.keyboard, target)?;
                    object_remove_flag(ui.keyboard, ObjectFlag::Hidden)?;
                }
            } else if code == EventCode::ValueChanged as u32
                && target == ui.text
                && ui.working_unusable
            {
                ui.revision = ui.revision.wrapping_add(1);
                ui.confirm_open = None;
                ui.confirm_close = None;
            } else if (code == EventCode::Ready as u32 || code == EventCode::Cancel as u32)
                && target == ui.keyboard
            {
                unsafe {
                    object_add_flag(ui.keyboard, ObjectFlag::Hidden)?;
                }
            }
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
                c"Te".as_ptr(),
                Color {
                    red: 76,
                    green: 110,
                    blue: 245,
                },
            )?;
            object_set_flex_flow(window, FlexFlow::Column)?;
            object_set_padding(window, 4)?;
            let actions = buttonmatrix_create(window)?;
            object_set_width(actions, percentage(100)?)?;
            object_set_height(actions, 36)?;
            let action_map = [
                translate!(c"navigation.previous_page").as_ptr(),
                translate!(c"navigation.next_page").as_ptr(),
                c"\n".as_ptr(),
                translate!(c"controls.new_file").as_ptr(),
                translate!(c"controls.open_file").as_ptr(),
                translate!(c"controls.save_file").as_ptr(),
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
            let page_label = label_create(window)?;
            object_set_width(page_label, percentage(100)?)?;
            let text = textarea_create(window)?;
            textarea_set_max_length(text, PAGE_BYTES as u32)?;
            object_set_width(text, percentage(100)?)?;
            object_set_flex_grow(text, 1)?;
            let status = label_create(window)?;
            object_set_width(status, percentage(100)?)?;
            let keyboard = keyboard_create(window)?;
            object_set_height(keyboard, 110)?;
            object_add_flag(keyboard, ObjectFlag::Hidden)?;
            for object in [actions, path, text, keyboard] {
                object_add_flag(object, ObjectFlag::EventBubble)?;
            }
            let mut ui = Self {
                window,
                actions,
                path,
                text,
                keyboard,
                status,
                page_label,
                working: None,
                original_page: String::new(),
                page_start: 0,
                page_length: 0,
                dirty: false,
                revision: 0,
                confirm_open: None,
                confirm_close: None,
                working_unusable: false,
                _action_map: action_map,
            };
            ui.status(translate!("file_path.instructions"))?;
            ui.show_page()?;
            Ok(ui)
        }
    }

    fn path(&self) -> wasm::Result<String> {
        let mut bytes = [0i8; 240 * 4 + 1];
        unsafe {
            textarea_get_text(self.path, bytes.as_mut_ptr(), bytes.len())?;
        }
        let path = unsafe { CStr::from_ptr(bytes.as_ptr()) }
            .to_str()
            .unwrap_or("");
        if !path.starts_with('/') || path.ends_with('/') {
            return Err(-1);
        }
        Ok(path.to_owned())
    }

    fn edited_text(&self) -> wasm::Result<Option<String>> {
        let mut bytes = vec![0i8; PAGE_BYTES * 4 + 1];
        unsafe {
            textarea_get_text(self.text, bytes.as_mut_ptr(), bytes.len())?;
        }
        let text = unsafe { CStr::from_ptr(bytes.as_ptr()) }
            .to_str()
            .unwrap_or("");
        if text.len() > PAGE_BYTES || text.contains('\0') {
            return Ok(None);
        }
        Ok(Some(text.to_owned()))
    }

    fn commit_page(&mut self) -> wasm::Result<bool> {
        if self.working_unusable {
            return Ok(false);
        }
        if self.working.is_none() {
            return Ok(true);
        }
        let Some(text) = self.edited_text()? else {
            self.status(translate!("errors.page_exceeds_byte_limit"))?;
            return Ok(false);
        };
        if text != self.original_page {
            let result = self.working.as_mut().ok_or(-1)?.replace_page(
                self.page_start,
                self.page_length,
                &text,
            );
            if result.is_err() {
                self.dirty = true;
                self.revision = self.revision.wrapping_add(1);
                self.working_unusable = true;
                self.status(translate!("errors.edit_failed_reopen_source"))?;
                return Ok(false);
            }
            self.dirty = true;
            self.revision = self.revision.wrapping_add(1);
            self.show_page()?;
        }
        Ok(true)
    }

    fn navigate(&mut self, forward: bool) -> wasm::Result<()> {
        if !self.commit_page()? {
            return Ok(());
        }
        let Some(working) = self.working.as_mut() else {
            self.status(translate!("status.open_or_create_first"))?;
            return Ok(());
        };
        if forward {
            let next = self.page_start + self.page_length as u64;
            if next < working.length() {
                self.page_start = next;
            }
        } else if self.page_start > 0 {
            self.page_start = working
                .previous_page_start(self.page_start)
                .map_err(|_| -1)?;
        }
        self.show_page()?;
        Ok(())
    }

    fn new_document(&mut self) -> wasm::Result<()> {
        if (!self.working_unusable && !self.commit_page()?) || !self.allow_discard()? {
            return Ok(());
        }
        let path = match self.path() {
            Ok(path) => path,
            Err(_) => {
                self.status(translate!("file_path.placeholder"))?;
                return Ok(());
            }
        };
        let parent = Path::new(&path).parent().ok_or(-1)?;
        match WorkingCopy::create(parent) {
            Ok(working) => {
                self.working = Some(working);
                self.page_start = 0;
                self.page_length = 0;
                self.original_page.clear();
                self.dirty = true;
                self.working_unusable = false;
                self.revision = self.revision.wrapping_add(1);
                self.show_page()?;
                self.status(translate!("file_path.new_file_default_name"))?;
            }
            Err(_) => self.status(translate!("errors.working_copy_creation_failed"))?,
        }
        Ok(())
    }

    fn open_document(&mut self) -> wasm::Result<()> {
        if (!self.working_unusable && !self.commit_page()?) || !self.allow_discard()? {
            return Ok(());
        }
        let path = match self.path() {
            Ok(path) => path,
            Err(_) => {
                self.status(translate!("file_path.placeholder"))?;
                return Ok(());
            }
        };
        match WorkingCopy::open(&path) {
            Ok(working) => {
                self.working = Some(working);
                self.page_start = 0;
                self.page_length = 0;
                self.original_page.clear();
                self.dirty = false;
                self.working_unusable = false;
                self.revision = self.revision.wrapping_add(1);
                self.show_page()?;
                self.status(translate!("status.opened"))?;
            }
            Err(_) => self.status(translate!("errors.open_failed"))?,
        }
        Ok(())
    }

    fn save_document(&mut self) -> wasm::Result<()> {
        if self.working_unusable {
            self.status(translate!("errors.edit_failed_reopen_source"))?;
            return Ok(());
        }
        if !self.commit_page()? {
            return Ok(());
        }
        let path = match self.path() {
            Ok(path) => path,
            Err(_) => {
                self.status(translate!("file_path.placeholder"))?;
                return Ok(());
            }
        };
        let save_result = match self.working.as_mut() {
            Some(working) => working.save_to(&path),
            None => return self.status(translate!("status.open_or_create_first")),
        };
        if save_result.is_ok() {
            self.dirty = false;
            self.revision = self.revision.wrapping_add(1);
            self.status(translate!("status.saved"))?;
        } else {
            self.status(translate!("errors.save_failed_working_copy_preserved"))?;
        }
        Ok(())
    }

    fn allow_discard(&mut self) -> wasm::Result<bool> {
        if self.dirty && self.confirm_open != Some(self.revision) {
            self.confirm_open = Some(self.revision);
            self.status(translate!("warnings.unsaved_new_or_open_again"))?;
            return Ok(false);
        }
        self.confirm_open = None;
        Ok(true)
    }

    fn show_page(&mut self) -> wasm::Result<()> {
        let mut bytes = [0; PAGE_BYTES];
        let mut page = Page { length: 0 };
        if let Some(working) = self.working.as_mut() {
            page = working
                .read_page(self.page_start, &mut bytes)
                .map_err(|_| -1)?;
        }
        let page_text = std::str::from_utf8(&bytes[..page.length]).map_err(|_| -1)?;
        self.original_page = page_text.to_owned();
        let text = CString::new(page_text).map_err(|_| -1)?;
        unsafe {
            textarea_set_text(self.text, text.as_ptr())?;
            let total_length = self.working.as_ref().map_or(0, WorkingCopy::length);
            let label = CString::new(format!(
                "{} | {} of {} bytes",
                self.page_start, page.length, total_length
            ))
            .map_err(|_| -1)?;
            label_set_text(self.page_label, label.as_ptr().cast_mut())?;
        }
        self.page_length = page.length;
        Ok(())
    }

    fn status(&self, text: &str) -> wasm::Result<()> {
        let text = CString::new(text).map_err(|_| -1)?;
        unsafe { label_set_text(self.status, text.as_ptr().cast_mut()) }
    }
}
