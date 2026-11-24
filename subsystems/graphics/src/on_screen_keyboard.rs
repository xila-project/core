use core::{cell::Cell, ptr};

use crate::lvgl::{self, *};

pub(crate) struct OnScreenKeyboard {
    keyboard: Cell<*mut lv_obj_t>,
    target: Cell<*mut lv_obj_t>,
    viewport: Cell<*mut lv_obj_t>,
    original_padding: Cell<Option<i32>>,
}

impl OnScreenKeyboard {
    pub(crate) fn new() -> Self {
        Self {
            keyboard: Cell::new(ptr::null_mut()),
            target: Cell::new(ptr::null_mut()),
            viewport: Cell::new(ptr::null_mut()),
            original_padding: Cell::new(None),
        }
    }

    /// The manager holds the graphics lock and keeps this state alive through
    /// display teardown. The address must remain stable while callbacks exist.
    pub(crate) unsafe fn observe(&self, display: *mut lv_display_t) {
        unsafe {
            let data = ptr::from_ref(self).cast_mut().cast();
            lv_display_add_event_cb(
                display,
                Some(screen_loaded),
                lv_event_code_t_LV_EVENT_SCREEN_LOADED,
                data,
            );
            self.observe_root(lv_display_get_screen_active(display));
        }
    }

    unsafe fn observe_root(&self, root: *mut lv_obj_t) {
        unsafe {
            let data = ptr::from_ref(self).cast_mut().cast();
            lv_obj_remove_event_cb_with_user_data(root, Some(root_event), data);
            for code in [
                lv_event_code_t_LV_EVENT_CHILD_CREATED,
                lv_event_code_t_LV_EVENT_DELETE,
            ] {
                lv_obj_add_event_cb(root, Some(root_event), code, data);
            }
            self.observe_subtree(root);
        }
    }

    unsafe fn observe_subtree(&self, object: *mut lv_obj_t) {
        unsafe {
            if lv_obj_has_class(object, &lv_textarea_class) {
                self.observe_textarea(object);
            }
            for index in 0..lv_obj_get_child_count(object) {
                self.observe_subtree(lv_obj_get_child(object, index as i32));
            }
        }
    }

    unsafe fn observe_textarea(&self, object: *mut lv_obj_t) {
        unsafe {
            let data = ptr::from_ref(self).cast_mut().cast();
            lv_obj_remove_event_cb_with_user_data(object, Some(textarea_event), data);
            for code in [
                lv_event_code_t_LV_EVENT_CLICKED,
                lv_event_code_t_LV_EVENT_DEFOCUSED,
                lv_event_code_t_LV_EVENT_LEAVE,
                lv_event_code_t_LV_EVENT_READY,
                lv_event_code_t_LV_EVENT_CANCEL,
                lv_event_code_t_LV_EVENT_DELETE,
            ] {
                lv_obj_add_event_cb(object, Some(textarea_event), code, data);
            }
        }
    }

    unsafe fn dismiss(&self, target: *mut lv_obj_t) {
        if self.target.get() == target {
            self.target.set(ptr::null_mut());
        }
        unsafe {
            let keyboard = self.keyboard.get();
            if !keyboard.is_null() && lv_keyboard_get_textarea(keyboard) == target {
                // DELETE is sent before the textarea's storage is freed.
                lv_keyboard_set_textarea(keyboard, ptr::null_mut());
                lv_obj_add_flag(keyboard, lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            }
        }
    }

    pub(crate) unsafe fn update(&self, display: *mut lv_display_t) {
        unsafe {
            let target = self.target.get();
            let screen = lv_display_get_screen_active(display);
            if !target.is_null() {
                let root = lv_obj_get_screen(target);
                let mut ancestor = target;
                while !ancestor.is_null()
                    && !lv_obj_has_flag(ancestor, lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN)
                {
                    ancestor = lv_obj_get_parent(ancestor);
                }
                if root != screen || !ancestor.is_null() {
                    self.dismiss(target);
                }
            }

            let target = self.target.get();
            let keyboard = self.keyboard.get();
            if target.is_null() {
                self.restore_viewport();
                return;
            }
            if !keyboard.is_null() && lv_keyboard_get_textarea(keyboard) == target {
                return;
            }

            self.restore_viewport();
            let keyboard = if keyboard.is_null() {
                let keyboard = lv_keyboard_create(lv_display_get_layer_top(display));
                if keyboard.is_null() {
                    return;
                }
                self.keyboard.set(keyboard);
                lv_obj_align(keyboard, lv_align_t_LV_ALIGN_BOTTOM_MID, 0, 0);
                lv_obj_remove_flag(keyboard, lv_obj_flag_t_LV_OBJ_FLAG_CLICK_FOCUSABLE);
                lv_obj_add_event_cb(
                    keyboard,
                    Some(keyboard_deleted),
                    lv_event_code_t_LV_EVENT_DELETE,
                    ptr::from_ref(self).cast_mut().cast(),
                );
                keyboard
            } else {
                keyboard
            };
            let height = lv_display_get_horizontal_resolution(display) / 3;
            lv_obj_set_size(keyboard, lv_pct(100), height);
            lv_keyboard_set_textarea(keyboard, target);
            lv_obj_remove_flag(keyboard, lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            lv_obj_move_foreground(keyboard);
            lv_obj_update_layout(keyboard);
            if self.target.get() != target || self.keyboard.get() != keyboard {
                return;
            }

            let viewport = lv_obj_get_screen(target);
            let mut original = lv_style_value_t::default();
            let result = lv_obj_get_local_style_prop(
                viewport,
                _lv_style_id_t_LV_STYLE_PAD_BOTTOM as lv_style_prop_t,
                &mut original,
                0,
            );
            self.original_padding
                .set((result == lv_style_res_t_LV_STYLE_RES_FOUND).then_some(original.num));
            self.viewport.set(viewport);
            let padding = lv_obj_get_style_pad_bottom(viewport, 0);
            lv_obj_set_style_pad_bottom(viewport, padding + lv_obj_get_height(keyboard), 0);
            lv_obj_update_layout(viewport);
            if self.target.get() == target {
                lv_obj_scroll_to_view_recursive(target, false);
            }
        }
    }

    unsafe fn restore_viewport(&self) {
        let viewport = self.viewport.replace(ptr::null_mut());
        if viewport.is_null() {
            return;
        }
        unsafe {
            if let Some(padding) = self.original_padding.take() {
                lv_obj_set_style_pad_bottom(viewport, padding, 0);
            } else {
                lv_obj_remove_local_style_prop(
                    viewport,
                    _lv_style_id_t_LV_STYLE_PAD_BOTTOM as lv_style_prop_t,
                    0,
                );
            }
        }
    }
}

/// Whether this display has a system keyboard observer.
pub(crate) unsafe fn enabled_for(object: *mut lv_obj_t) -> bool {
    unsafe {
        let display = lv_obj_get_display(object);
        (0..lv_display_get_event_count(display)).any(|index| {
            let descriptor = lv_display_get_event_dsc(display, index);
            lv_event_dsc_get_cb(descriptor).is_some_and(|callback| {
                ptr::fn_addr_eq(callback, screen_loaded as unsafe extern "C" fn(_))
            })
        })
    }
}

unsafe extern "C" fn screen_loaded(event: *mut lv_event_t) {
    unsafe {
        let state = &*lv_event_get_user_data(event).cast::<OnScreenKeyboard>();
        state.observe_root(lv_event_get_param(event).cast());
    }
}

unsafe extern "C" fn root_event(event: *mut lv_event_t) {
    unsafe {
        let state = &*lv_event_get_user_data(event).cast::<OnScreenKeyboard>();
        match lv_event_get_code(event) {
            // LVGL always bubbles CHILD_CREATED through the ancestor chain.
            lvgl::lv_event_code_t_LV_EVENT_CHILD_CREATED => {
                let child = lv_event_get_param(event).cast::<lv_obj_t>();
                if !child.is_null() && lv_obj_has_class(child, &lv_textarea_class) {
                    state.observe_textarea(child);
                }
            }
            lvgl::lv_event_code_t_LV_EVENT_DELETE
                if state.viewport.get() == lv_event_get_target_obj(event) =>
            {
                state.viewport.set(ptr::null_mut());
            }
            _ => {}
        }
    }
}

unsafe extern "C" fn textarea_event(event: *mut lv_event_t) {
    unsafe {
        let state = &*lv_event_get_user_data(event).cast::<OnScreenKeyboard>();
        let target = lv_event_get_current_target_obj(event);
        if lv_event_get_target_obj(event) != target {
            return;
        }
        match lv_event_get_code(event) {
            // Only a tap opens the keyboard: the default group can focus a field
            // programmatically before any user input.
            lvgl::lv_event_code_t_LV_EVENT_CLICKED => {
                state.target.set(target);
            }
            lvgl::lv_event_code_t_LV_EVENT_DEFOCUSED
            | lvgl::lv_event_code_t_LV_EVENT_LEAVE
            | lvgl::lv_event_code_t_LV_EVENT_READY
            | lvgl::lv_event_code_t_LV_EVENT_CANCEL
            | lvgl::lv_event_code_t_LV_EVENT_DELETE => state.dismiss(target),
            _ => {}
        }
    }
}

unsafe extern "C" fn keyboard_deleted(event: *mut lv_event_t) {
    unsafe {
        let state = &*lv_event_get_user_data(event).cast::<OnScreenKeyboard>();
        state.keyboard.set(ptr::null_mut());
    }
}
