//! A graphics-owned, parent-lifetime file chooser. All VFS calls use its captured caller.
mod model;

use crate::{Error, Result, ffi};
use alloc::{boxed::Box, ffi::CString, format, string::String};
use core::{
    ffi::CStr,
    ptr::{self, NonNull},
};
use embedded_i18n::t;
use model::{Browser, MAX_PATH, Model, PAGE_SIZE};
use synchronization::once_lock::OnceLock;
use task::TaskIdentifier;
use virtual_file_system::VirtualFileSystem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FilePickerMode {
    OpenFile = 0,
    SaveFile = 1,
    Directory = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FilePickerStatus {
    Pending = 0,
    Selected = 1,
    Cancelled = 2,
}

const MAX_INPUTS: usize = 8;
struct ModalFocus {
    group: *mut ffi::lv_group_t,
    inputs: [(*mut ffi::lv_indev_t, *mut ffi::lv_group_t); MAX_INPUTS],
    count: usize,
}
impl ModalFocus {
    unsafe fn capture() -> Result<Self> {
        unsafe {
            let mut modal = Self {
                group: ptr::null_mut(),
                inputs: [(ptr::null_mut(), ptr::null_mut()); MAX_INPUTS],
                count: 0,
            };
            let mut input = ffi::lv_indev_get_next(ptr::null_mut());
            while !input.is_null() {
                if matches!(
                    ffi::lv_indev_get_type(input),
                    ffi::lv_indev_type_t_LV_INDEV_TYPE_KEYPAD
                        | ffi::lv_indev_type_t_LV_INDEV_TYPE_ENCODER
                ) {
                    if modal.count == MAX_INPUTS {
                        return Err(Error::NotAvailable);
                    }
                    modal.inputs[modal.count] = (input, ffi::lv_indev_get_group(input));
                    modal.count += 1;
                }
                input = ffi::lv_indev_get_next(input);
            }
            modal.group = ffi::lv_group_create();
            if modal.group.is_null() {
                return Err(Error::OutOfMemory);
            }
            Ok(modal)
        }
    }
    unsafe fn activate(&self) {
        for (input, _) in &self.inputs[..self.count] {
            unsafe {
                ffi::lv_indev_set_group(*input, self.group);
            }
        }
    }
}
impl ModalFocus {
    fn release(&mut self) {
        unsafe {
            if self.group.is_null() {
                return;
            }
            for (saved_input, saved_group) in &self.inputs[..self.count] {
                let mut input = ffi::lv_indev_get_next(ptr::null_mut());
                while !input.is_null() {
                    if input == *saved_input && ffi::lv_indev_get_group(input) == self.group {
                        let mut group = if saved_group.is_null() {
                            ptr::null_mut()
                        } else {
                            ffi::lv_group_get_default()
                        };
                        for index in 0..ffi::lv_group_get_count() {
                            if ffi::lv_group_by_index(index) == *saved_group {
                                group = *saved_group;
                                break;
                            }
                        }
                        ffi::lv_indev_set_group(input, group);
                        break;
                    }
                    input = ffi::lv_indev_get_next(input);
                }
            }
            ffi::lv_group_delete(self.group);
            self.group = ptr::null_mut();
        }
    }
}
impl Drop for ModalFocus {
    fn drop(&mut self) {
        self.release();
    }
}
struct State {
    _modal: ModalFocus,
    vfs: &'static VirtualFileSystem,
    model: Model,
    path: *mut ffi::lv_obj_t,
    error: *mut ffi::lv_obj_t,
    keyboard: *mut ffi::lv_obj_t,
    up: *mut ffi::lv_obj_t,
    previous: *mut ffi::lv_obj_t,
    next: *mut ffi::lv_obj_t,
    confirm: *mut ffi::lv_obj_t,
    cancel: *mut ffi::lv_obj_t,
    rows: [*mut ffi::lv_obj_t; PAGE_SIZE],
}

#[repr(C)]
pub struct FilePicker {
    object: ffi::lv_obj_t,
    state: Option<Box<State>>,
}
struct Class(ffi::lv_obj_class_t);
unsafe impl Send for Class {}
unsafe impl Sync for Class {}
static CLASS: OnceLock<Class> = OnceLock::new();
pub fn get_file_picker_class() -> &'static ffi::lv_obj_class_t {
    &CLASS
        .get_or_init(|| {
            let mut class = ffi::lv_obj_class_t {
                base_class: unsafe { &ffi::lv_obj_class },
                constructor_cb: Some(construct),
                destructor_cb: Some(destruct),
                width_def: unsafe { ffi::lv_pct(100) },
                height_def: unsafe { ffi::lv_pct(100) },
                name: c"file_picker".as_ptr(),
                ..Default::default()
            };
            class.set_instance_size(size_of::<FilePicker>() as _);
            Class(class)
        })
        .0
}
unsafe extern "C" fn construct(_: *const ffi::lv_obj_class_t, object: *mut ffi::lv_obj_t) {
    unsafe {
        ptr::write(&mut (*(object as *mut FilePicker)).state, None);
    }
}
unsafe extern "C" fn destruct(_: *const ffi::lv_obj_class_t, object: *mut ffi::lv_obj_t) {
    unsafe {
        let state = (*(object as *mut FilePicker)).state.take();
        drop(state);
    }
}
unsafe fn text(object: *mut ffi::lv_obj_t, value: &str) {
    if let Ok(value) = CString::new(value) {
        unsafe {
            ffi::lv_label_set_text(object, value.as_ptr());
        }
    }
}
unsafe fn button(
    parent: *mut ffi::lv_obj_t,
    value: &str,
    picker: *mut FilePicker,
) -> *mut ffi::lv_obj_t {
    unsafe {
        let object = ffi::lv_button_create(parent);
        let label = ffi::lv_label_create(object);
        text(label, value);
        ffi::lv_obj_add_event_cb(
            object,
            Some(callback),
            ffi::lv_event_code_t_LV_EVENT_CLICKED,
            picker.cast(),
        );
        ffi::lv_obj_add_event_cb(
            object,
            Some(callback),
            ffi::lv_event_code_t_LV_EVENT_KEY,
            picker.cast(),
        );
        object
    }
}
impl FilePicker {
    /// Create a picker under `parent`; deleting its LVGL parent also drops all Rust state.
    ///
    /// # Safety
    /// `parent` must be a live LVGL object and the graphics lock must be held.
    pub unsafe fn new(
        parent: *mut ffi::lv_obj_t,
        vfs: &'static VirtualFileSystem,
        caller: TaskIdentifier,
        mode: FilePickerMode,
        initial: &str,
    ) -> Result<NonNull<Self>> {
        let initial = model::normalize(initial).map_err(|_| Error::InvalidParameter)?;
        unsafe {
            let modal = ModalFocus::capture()?;
            let object = ffi::lv_obj_class_create_obj(get_file_picker_class(), parent);
            let mut picker =
                NonNull::new(object.cast::<Self>()).ok_or(Error::FailedToCreateObject)?;
            ffi::lv_obj_class_init_obj(object);
            ffi::lv_obj_add_flag(object, ffi::lv_obj_flag_t_LV_OBJ_FLAG_IGNORE_LAYOUT);
            ffi::lv_obj_remove_flag(object, ffi::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
            ffi::lv_obj_set_size(object, ffi::lv_pct(100), ffi::lv_pct(100));
            ffi::lv_obj_center(object);
            ffi::lv_obj_move_foreground(object);
            crate::theme::set_surface(object, crate::theme::Surface::Background);
            ffi::lv_obj_set_style_bg_opa(object, ffi::LV_OPA_COVER as _, 0);
            ffi::lv_obj_set_flex_flow(object, ffi::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            let path = ffi::lv_textarea_create(object);
            ffi::lv_textarea_set_one_line(path, true);
            ffi::lv_textarea_set_max_length(path, MAX_PATH as _);
            ffi::lv_obj_set_width(path, ffi::lv_pct(100));
            ffi::lv_obj_add_event_cb(
                path,
                Some(callback),
                ffi::lv_event_code_t_LV_EVENT_ALL,
                picker.as_ptr().cast(),
            );
            let toolbar = ffi::lv_obj_create(object);
            ffi::lv_obj_set_size(toolbar, ffi::lv_pct(100), ffi::LV_SIZE_CONTENT as _);
            ffi::lv_obj_set_flex_flow(toolbar, ffi::lv_flex_flow_t_LV_FLEX_FLOW_ROW_WRAP);
            let up = button(toolbar, t!("file_picker.parent"), picker.as_ptr());
            let previous = button(toolbar, t!("file_picker.previous"), picker.as_ptr());
            let next = button(toolbar, t!("file_picker.next"), picker.as_ptr());
            let confirm = button(
                toolbar,
                match mode {
                    FilePickerMode::OpenFile => t!("file_picker.open"),
                    FilePickerMode::SaveFile => t!("file_picker.save"),
                    FilePickerMode::Directory => t!("file_picker.select_directory"),
                },
                picker.as_ptr(),
            );
            let cancel = button(toolbar, t!("file_picker.cancel"), picker.as_ptr());
            let error = ffi::lv_label_create(object);
            ffi::lv_obj_set_width(error, ffi::lv_pct(100));
            let list = ffi::lv_obj_create(object);
            ffi::lv_obj_set_width(list, ffi::lv_pct(100));
            ffi::lv_obj_set_flex_grow(list, 1);
            ffi::lv_obj_set_flex_flow(list, ffi::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            let mut rows = [ptr::null_mut(); PAGE_SIZE];
            for row in &mut rows {
                *row = button(list, "", picker.as_ptr());
                ffi::lv_obj_set_width(*row, ffi::lv_pct(100));
            }
            let keyboard = ffi::lv_keyboard_create(object);
            ffi::lv_obj_set_size(keyboard, ffi::lv_pct(100), 160);
            ffi::lv_obj_add_flag(keyboard, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            ffi::lv_obj_add_event_cb(
                keyboard,
                Some(callback),
                ffi::lv_event_code_t_LV_EVENT_ALL,
                picker.as_ptr().cast(),
            );
            ffi::lv_obj_add_event_cb(
                object,
                Some(callback),
                ffi::lv_event_code_t_LV_EVENT_KEY,
                picker.as_ptr().cast(),
            );
            for control in [path, up, previous, next, confirm, cancel, keyboard]
                .into_iter()
                .chain(rows.iter().copied())
            {
                ffi::lv_group_add_obj(modal.group, control);
            }
            modal.activate();
            let state = Box::new(State {
                _modal: modal,
                vfs,
                model: Model::new(caller, mode),
                path,
                error,
                keyboard,
                up,
                previous,
                next,
                confirm,
                cancel,
                rows,
            });
            picker.as_mut().state = Some(state);
            picker.as_mut().reset(mode, &initial)?;
            Ok(picker)
        }
    }
    /// Reuse this picker and its guest handle for another selection session.
    /// Clears the previous result while preserving the original VFS and caller.
    ///
    /// # Safety
    /// The graphics lock must be held and this must be a live LVGL picker.
    pub unsafe fn reset(&mut self, mode: FilePickerMode, initial: &str) -> Result<()> {
        let initial = model::normalize(initial).map_err(|_| Error::InvalidParameter)?;
        unsafe {
            let state = self.state.as_mut().ok_or(Error::InvalidReference)?;
            if state._modal.group.is_null() {
                let modal = ModalFocus::capture()?;
                for control in [
                    state.path,
                    state.up,
                    state.previous,
                    state.next,
                    state.confirm,
                    state.cancel,
                    state.keyboard,
                ]
                .into_iter()
                .chain(state.rows.iter().copied())
                {
                    ffi::lv_group_add_obj(modal.group, control);
                }
                state._modal = modal;
            }
            state.model = Model::new(state.model.caller, mode);
            let directory = match state.vfs.kind(state.model.caller, &initial) {
                Ok(file_system::Kind::Directory) => initial.as_str(),
                _ => model::parent(&initial),
            };
            let result = state.model.navigate(state.vfs, directory, 0);
            text(
                ffi::lv_obj_get_child(state.confirm, 0),
                match mode {
                    FilePickerMode::OpenFile => t!("file_picker.open"),
                    FilePickerMode::SaveFile => t!("file_picker.save"),
                    FilePickerMode::Directory => t!("file_picker.select_directory"),
                },
            );
            state._modal.activate();
            let path = state.path;
            ffi::lv_keyboard_set_textarea(state.keyboard, ptr::null_mut());
            ffi::lv_obj_add_flag(state.keyboard, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            self.refresh();
            ffi::lv_textarea_set_text(path, CString::new(initial).unwrap().as_ptr());
            self.show_result(result);
            ffi::lv_obj_remove_flag(&mut self.object, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            ffi::lv_obj_move_foreground(&mut self.object);
            Ok(())
        }
    }
    pub fn status(&self) -> FilePickerStatus {
        self.state
            .as_ref()
            .map_or(FilePickerStatus::Pending, |s| s.model.status)
    }
    pub fn selected_path(&self) -> Option<&str> {
        self.state.as_ref()?.model.selected.as_deref()
    }
    pub fn as_object_mutable(&mut self) -> &mut ffi::lv_obj_t {
        &mut self.object
    }
    unsafe fn finish(&mut self) {
        unsafe {
            if let Some(state) = &mut self.state {
                state._modal.release();
                ffi::lv_keyboard_set_textarea(state.keyboard, ptr::null_mut());
                ffi::lv_obj_add_flag(state.keyboard, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            }
            ffi::lv_obj_add_flag(&mut self.object, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
        }
    }
    unsafe fn show_result(&mut self, result: virtual_file_system::Result<()>) {
        if let Some(state) = &self.state {
            let message = result
                .err()
                .map(|error| format!("{}: {}", t!("file_picker.error"), error))
                .unwrap_or_default();
            unsafe {
                text(state.error, &message);
            }
        }
    }
    unsafe fn refresh(&mut self) {
        let Some(state) = &mut self.state else {
            return;
        };
        unsafe {
            ffi::lv_textarea_set_text(
                state.path,
                CString::new(state.model.directory.as_str())
                    .unwrap()
                    .as_ptr(),
            );
            for (index, row) in state.rows.iter().enumerate() {
                if let Some(entry) = state.model.page.entries.get(index) {
                    ffi::lv_obj_remove_flag(*row, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
                    text(
                        ffi::lv_obj_get_child(*row, 0),
                        &format!(
                            "{}{}",
                            entry.name,
                            if entry.kind == file_system::Kind::Directory {
                                "/"
                            } else {
                                ""
                            }
                        ),
                    );
                } else {
                    ffi::lv_obj_add_flag(*row, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
                }
            }
            for (button, enabled) in [
                (state.previous, state.model.offset > 0),
                (state.next, state.model.page.more),
            ] {
                if enabled {
                    ffi::lv_obj_remove_state(button, ffi::lv_state_t_LV_STATE_DISABLED as _);
                } else {
                    ffi::lv_obj_add_state(button, ffi::lv_state_t_LV_STATE_DISABLED as _);
                }
            }
        }
    }
}
unsafe extern "C" fn callback(event: *mut ffi::lv_event_t) {
    unsafe {
        let code = ffi::lv_event_get_code(event);
        if !matches!(
            code,
            ffi::lv_event_code_t_LV_EVENT_CLICKED
                | ffi::lv_event_code_t_LV_EVENT_FOCUSED
                | ffi::lv_event_code_t_LV_EVENT_READY
                | ffi::lv_event_code_t_LV_EVENT_CANCEL
                | ffi::lv_event_code_t_LV_EVENT_KEY
        ) {
            return;
        }
        let picker = ffi::lv_event_get_user_data(event).cast::<FilePicker>();
        if picker.is_null() {
            return;
        }
        let picker = &mut *picker;
        let Some(state) = &mut picker.state else {
            return;
        };
        if state.model.status != FilePickerStatus::Pending {
            return;
        }
        let target = ffi::lv_event_get_target_obj(event);
        let code = ffi::lv_event_get_code(event);
        if code == ffi::lv_event_code_t_LV_EVENT_KEY
            && ffi::lv_event_get_key(event) == ffi::lv_key_t_LV_KEY_ESC
        {
            state.model.cancel();
            picker.finish();
            return;
        }
        if target == state.path && code == ffi::lv_event_code_t_LV_EVENT_FOCUSED {
            ffi::lv_keyboard_set_textarea(state.keyboard, state.path);
            ffi::lv_obj_remove_flag(state.keyboard, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            return;
        }
        if target == state.keyboard
            && (code == ffi::lv_event_code_t_LV_EVENT_READY
                || code == ffi::lv_event_code_t_LV_EVENT_CANCEL)
        {
            ffi::lv_keyboard_set_textarea(state.keyboard, ptr::null_mut());
            ffi::lv_obj_add_flag(state.keyboard, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            if code == ffi::lv_event_code_t_LV_EVENT_CANCEL {
                return;
            }
        } else if target == state.keyboard
            || (code != ffi::lv_event_code_t_LV_EVENT_CLICKED
                && !(target == state.path && code == ffi::lv_event_code_t_LV_EVENT_READY))
        {
            return;
        }
        if target == state.path && code != ffi::lv_event_code_t_LV_EVENT_READY {
            return;
        }
        let path = CStr::from_ptr(ffi::lv_textarea_get_text(state.path))
            .to_str()
            .map(String::from);
        let mut navigated = false;
        let result = if target == state.cancel {
            state.model.cancel();
            Ok(())
        } else if target == state.confirm
            || ((target == state.path || target == state.keyboard)
                && code == ffi::lv_event_code_t_LV_EVENT_READY)
        {
            path.map_err(|_| virtual_file_system::Error::InvalidPath)
                .and_then(|path| state.model.activate(state.vfs, &path))
                .map(|did_navigate| {
                    navigated = did_navigate;
                })
        } else {
            let (destination, offset) = if target == state.up {
                (String::from(model::parent(&state.model.directory)), 0)
            } else if target == state.previous {
                (
                    state.model.directory.clone(),
                    state.model.offset.saturating_sub(PAGE_SIZE),
                )
            } else if target == state.next {
                (
                    state.model.directory.clone(),
                    state.model.offset.saturating_add(PAGE_SIZE),
                )
            } else if let Some(index) = state.rows.iter().position(|row| *row == target) {
                let Some(entry) = state.model.page.entries.get(index) else {
                    return;
                };
                let destination = format!(
                    "{}/{}",
                    state.model.directory.trim_end_matches('/'),
                    entry.name
                );
                let destination = match model::normalize(&destination) {
                    Ok(destination) => destination,
                    Err(error) => {
                        picker.show_result(Err(error));
                        return;
                    }
                };
                match state.vfs.kind(state.model.caller, &destination) {
                    Ok(file_system::Kind::Directory) => {}
                    Ok(_) => {
                        if let Ok(value) = CString::new(destination) {
                            ffi::lv_textarea_set_text(state.path, value.as_ptr());
                        }
                        return;
                    }
                    Err(error) => {
                        picker.show_result(Err(error));
                        return;
                    }
                }
                (destination, 0)
            } else {
                match path {
                    Ok(path) => (path, 0),
                    Err(_) => {
                        picker.show_result(Err(virtual_file_system::Error::InvalidPath));
                        return;
                    }
                }
            };
            let result = state.model.navigate(state.vfs, &destination, offset);
            navigated = result.is_ok();
            result
        };
        if navigated {
            picker.refresh();
        }
        picker.show_result(result);
        if picker.status() != FilePickerStatus::Pending {
            picker.finish();
        }
    }
}
