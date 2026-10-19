use core::ffi::c_char;

pub use lvgl_rust_sys::*;

use crate::Point;

pub const LV_SIZE_CONTENT: i32 = (LV_COORD_MAX | LV_COORD_TYPE_SPEC) as i32;

pub const LV_STATE_DEFAULT: u32 = 0;
pub const LV_STATE_CHECKED: lv_state_t = lv_state_t_LV_STATE_CHECKED;
pub const LV_STATE_PRESSED: lv_state_t = lv_state_t_LV_STATE_PRESSED;

pub const LV_PART_MAIN: u32 = 0;
pub const LV_PART_INDICATOR: u32 = 0x020000;
pub const LV_PART_KNOB: u32 = 0x030000;
pub const LV_PART_ITEMS: u32 = 0x050000;

pub const LV_OPA_TRANSP: u8 = 0;
pub const LV_OPA_0: u8 = 0;
pub const LV_OPA_50: u8 = 127;
pub const LV_OPA_COVER: u8 = 255;

/// Set the padding of an object on all sides
///
/// # Arguments
///
/// * `Object` - The object to set the padding of.
/// * `Padding` - The padding to set.
/// * `Selector` - The selector to set the padding for.
///
/// # Safety
///
/// This function is unsafe because it may dereference raw pointers (e.g. `Object`).
pub unsafe fn lv_obj_set_style_pad_all(
    object: *mut lv_obj_t,
    padding: i32,
    selector: lv_style_selector_t,
) {
    unsafe {
        lv_obj_set_style_pad_top(object, padding, selector);
        lv_obj_set_style_pad_bottom(object, padding, selector);
        lv_obj_set_style_pad_left(object, padding, selector);
        lv_obj_set_style_pad_right(object, padding, selector);
    }
}

/// Set the padding of an object on the top side
///
/// # Arguments
///
/// * `Object` - The object to set the padding of.
/// * `Padding` - The padding to set.
/// * `Selector` - The selector to set the padding for.
///
/// # Safety
///
/// This function is unsafe because it may dereference raw pointers (e.g. `Object`).
///
pub unsafe fn lv_obj_move_foreground(object: *mut lv_obj_t) {
    unsafe {
        lv_obj_move_to_index(object, -1);
    }
}

/// Get the size of an object as a Point
///
/// # Arguments
/// * `Object` - The object to get the size of.
///
///  # Safety
/// This function is unsafe because it may dereference raw pointers (e.g. `Object`).
///
pub unsafe fn lv_obj_get_size(object: *mut lv_obj_t) -> Point {
    unsafe {
        let width = lv_obj_get_width(object) as i16;
        let height = lv_obj_get_height(object) as i16;

        Point::new(width, height)
    }
}

/// Add a tab to a tabview and adjust the tab button size
///
/// # Arguments
///
/// * `tabview` - The tabview to add the tab to.
/// * `name` - The name of the tab.
///
/// # Safety
///
/// This function is unsafe because it may dereference raw pointers (e.g. `tabview`).
pub unsafe fn lv_tabview_add_tab(tabview: *mut lv_obj_t, name: *const c_char) -> *mut lv_obj_t {
    unsafe {
        let page = lvgl_rust_sys::lv_tabview_add_tab(tabview, name);

        let bar = lv_tabview_get_tab_bar(tabview);

        lv_obj_set_size(bar, LV_SIZE_CONTENT, LV_SIZE_CONTENT);

        // get latest tab button
        let tab_count = lv_obj_get_child_count(bar);
        let button = lv_obj_get_child(bar, (tab_count - 1) as _);

        // don't make it grow
        lv_obj_set_flex_grow(button, 0);
        lv_obj_set_size(button, LV_SIZE_CONTENT, LV_SIZE_CONTENT);

        page
    }
}

unsafe extern "C" fn radio_event_handler(event: *mut lv_event_t) {
    unsafe {
        let code = lvgl_rust_sys::lv_event_get_code(event);
        let target = lvgl_rust_sys::lv_event_get_target(event) as *mut lv_obj_t;
        let user_data = lvgl_rust_sys::lv_event_get_user_data(event) as *mut lv_obj_t;

        if code == lv_event_code_t_LV_EVENT_CLICKED {
            let parent = user_data as *mut lv_obj_t;
            let child_count = lvgl_rust_sys::lv_obj_get_child_count(parent);

            for i in 0..child_count {
                let child = lvgl_rust_sys::lv_obj_get_child(parent, i as _);
                if child != target {
                    lvgl_rust_sys::lv_obj_remove_state(child, LV_STATE_CHECKED as _);
                }
            }

            lvgl_rust_sys::lv_obj_add_state(target, LV_STATE_CHECKED as _);
        }
    }
}

/// Create a radio button (a checkbox that behaves like a radio button)
///
/// # Arguments
///
/// * `parent` - The parent object of the radio button.
///
/// # Safety
/// This function is unsafe because it may dereference raw pointers (e.g. `parent`).
pub unsafe fn lv_radiobox_create(parent: *mut lv_obj_t) -> *mut lv_obj_t {
    unsafe {
        let checkbox = lvgl_rust_sys::lv_checkbox_create(parent);

        lvgl_rust_sys::lv_obj_add_event_cb(
            checkbox,
            Some(radio_event_handler),
            lvgl_rust_sys::lv_event_code_t_LV_EVENT_CLICKED,
            parent as _,
        );

        checkbox
    }
}
