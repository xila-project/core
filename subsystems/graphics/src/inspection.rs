use crate::ffi;
use alloc::{string::String, vec::Vec};
use core::str;
const UI_ELEMENT_MAX_TEXT_BYTES: usize = 512;
const UI_ELEMENT_MAX_NAME_BYTES: usize = 128;
const UI_NAME_MAX_CHILDREN: u32 = 16;
/// A bounded, pointer-free description of one LVGL object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiElement {
    /// Position in the depth-first snapshot, valid only for that snapshot.
    pub id: u32,
    pub parent_id: Option<u32>,
    /// LVGL class name, retained for compatibility and custom widgets.
    pub role: String,
    /// Semantic role for supported widgets; absent for unknown classes.
    pub semantic_role: Option<&'static str>,
    /// Direct label or placeholder, capped to 128 input bytes.
    pub name: Option<String>,
    /// Raw text from labels and text areas, as in the original snapshot API.
    pub text: Option<String>,
    /// The object or an ancestor is marked hidden (independent of clipping).
    pub hidden: bool,
    /// The object is marked disabled by LVGL.
    pub disabled: bool,
    pub focused: bool,
    /// LVGL clickable flag, not a guarantee that the object can be hit.
    pub clickable: bool,
    pub checked: Option<bool>,
    /// Inclusive screen coordinates `(x1, y1, x2, y2)`.
    pub bounds: (i32, i32, i32, i32),
    /// Geometry/hidden/opacity visibility, not a promise of being unoccluded.
    pub visible: bool,
    pub clipped_bounds: Option<(i32, i32, i32, i32)>,
    pub layer: &'static str,
    pub redacted: bool,
    pub text_truncated: bool,
    pub name_truncated: bool,
    pub state: u32,
    pub scroll: (i32, i32),
    pub value: Option<i32>,
    pub range: Option<(i32, i32)>,
    pub selected: Option<u32>,
    pub selected_text: Option<String>,
    pub selected_text_truncated: bool,
    pub style: UiStyle,
    pub buttons: Vec<UiButton>,
    pub buttons_truncated: bool,
}

/// One bounded UI-tree snapshot and whether additional objects were omitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiTreeSnapshot {
    pub elements: Vec<UiElement>,
    pub truncated: bool,
}

pub(crate) fn semantic_role(class: &str) -> Option<&'static str> {
    match class {
        "lv_button" | "lv_list_button" => Some("button"),
        "lv_textarea" => Some("textbox"),
        "lv_checkbox" => Some("checkbox"),
        "lv_switch" => Some("switch"),
        "lv_slider" => Some("slider"),
        "lv_dropdown" => Some("combobox"),
        "lv_label" | "lv_list_text" => Some("text"),
        "lv_list" => Some("list"),
        "lv_buttonmatrix" | "lv_keyboard" => Some("buttonmatrix"),
        "lv_bar" => Some("progressbar"),
        "lv_arc" => Some("dial"),
        "lv_roller" => Some("listbox"),
        "lv_spinbox" => Some("spinbutton"),
        "lv_tabview" => Some("tablist"),
        "lv_tileview" => Some("tileview"),
        "lv_calendar" => Some("calendar"),
        "lv_chart" => Some("chart"),
        "lv_canvas" => Some("canvas"),
        "lv_table" => Some("table"),
        "lv_image" => Some("image"),
        "lv_menu" => Some("menu"),
        "lv_msgbox" => Some("dialog"),
        _ => None,
    }
}

/// Flags report explicit LVGL state; they do not assert visibility through
/// clipping, occlusion, or whether a click would hit this object.
unsafe fn is_hidden_by_ancestor(mut object: *mut ffi::lv_obj_t) -> bool {
    while !object.is_null() {
        if unsafe { ffi::lv_obj_has_flag(object, ffi::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN) } {
            return true;
        }
        object = unsafe { ffi::lv_obj_get_parent(object) };
    }
    false
}

/// Use only unambiguous associations: textarea placeholder, checkbox text, or
/// the first direct label in a button. Never search arbitrary siblings.
unsafe fn widget_name(object: *mut ffi::lv_obj_t, semantic_role: Option<&str>) -> Option<String> {
    unsafe {
        match semantic_role {
            Some("textbox") if ffi::lv_obj_has_class(object, &ffi::lv_textarea_class) => {
                bounded_c_string_with_limit(
                    ffi::lv_textarea_get_placeholder_text(object),
                    UI_ELEMENT_MAX_NAME_BYTES,
                )
            }
            Some("checkbox") if ffi::lv_obj_has_class(object, &ffi::lv_checkbox_class) => {
                bounded_c_string_with_limit(
                    ffi::lv_checkbox_get_text(object),
                    UI_ELEMENT_MAX_NAME_BYTES,
                )
            }
            Some("button") if ffi::lv_obj_has_class(object, &ffi::lv_button_class) => {
                for index in 0..ffi::lv_obj_get_child_count(object).min(UI_NAME_MAX_CHILDREN) {
                    let child = ffi::lv_obj_get_child(object, index as i32);
                    if ffi::lv_obj_has_class(child, &ffi::lv_label_class) {
                        return bounded_c_string_with_limit(
                            ffi::lv_label_get_text(child),
                            UI_ELEMENT_MAX_NAME_BYTES,
                        );
                    }
                }
                None
            }
            _ => None,
        }
    }
}

pub(crate) unsafe fn inspect_children(
    object: *mut ffi::lv_obj_t,
    parent_id: Option<u32>,
    depth: usize,
    limits: (usize, usize),
    elements: &mut Vec<UiElement>,
    truncated: &mut bool,
    layer: &'static str,
) {
    let (max_depth, max_elements) = limits;
    if object.is_null() {
        return;
    }
    if depth >= max_depth || elements.len() >= max_elements {
        *truncated = true;
        return;
    }

    let id = elements.len() as u32;
    let class = unsafe { ffi::lv_obj_get_class(object) };
    let role = if class.is_null() {
        String::from("unknown")
    } else {
        unsafe { bounded_c_string((*class).name) }.unwrap_or_else(|| String::from("unknown"))
    };

    let mut area = ffi::lv_area_t::default();
    unsafe { ffi::lv_obj_get_coords(object, &mut area) };

    let redacted = unsafe { password_ancestor(object) };
    let text = if redacted {
        None
    } else {
        unsafe {
            if ffi::lv_obj_has_class(object, &ffi::lv_label_class) {
                bounded_c_string(ffi::lv_label_get_text(object))
            } else if ffi::lv_obj_has_class(object, &ffi::lv_textarea_class) {
                bounded_c_string(ffi::lv_textarea_get_text(object))
            } else {
                None
            }
        }
    };

    let semantic_role = semantic_role(&role);
    let (name, hidden, disabled, focused, clickable, checked) = unsafe {
        (
            if redacted && !ffi::lv_obj_has_class(object, &ffi::lv_textarea_class) {
                None
            } else {
                widget_name(object, semantic_role)
            },
            is_hidden_by_ancestor(object),
            ffi::lv_obj_has_state(object, ffi::lv_state_t_LV_STATE_DISABLED),
            ffi::lv_obj_has_state(object, ffi::lv_state_t_LV_STATE_FOCUSED),
            ffi::lv_obj_has_flag(object, ffi::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE),
            if matches!(semantic_role, Some("checkbox" | "switch")) {
                Some(ffi::lv_obj_has_state(
                    object,
                    ffi::lv_state_t_LV_STATE_CHECKED,
                ))
            } else {
                None
            },
        )
    };

    let clipped_bounds = unsafe { clipped_bounds(object) };
    let visible = !hidden
        && clipped_bounds.is_some()
        && unsafe { ffi::lv_obj_get_style_opa_recursive(object, 0) } != 0;
    let WidgetValue {
        value,
        range,
        selected,
        selected_text,
        selected_text_truncated,
    } = if redacted {
        WidgetValue::default()
    } else {
        unsafe { widget_value(object, &role) }
    };
    let (buttons, buttons_truncated) = if redacted {
        (Vec::new(), false)
    } else {
        unsafe {
            matrix_buttons(
                object,
                &role,
                128usize.saturating_sub(elements.iter().map(|e| e.buttons.len()).sum::<usize>()),
            )
        }
    };
    let text_truncated = !redacted
        && unsafe {
            if ffi::lv_obj_has_class(object, &ffi::lv_label_class) {
                was_capped(ffi::lv_label_get_text(object), UI_ELEMENT_MAX_TEXT_BYTES)
            } else if ffi::lv_obj_has_class(object, &ffi::lv_textarea_class) {
                was_capped(ffi::lv_textarea_get_text(object), UI_ELEMENT_MAX_TEXT_BYTES)
            } else {
                false
            }
        };
    let name_truncated = name.is_some()
        && unsafe {
            match semantic_role {
                Some("textbox") => was_capped(
                    ffi::lv_textarea_get_placeholder_text(object),
                    UI_ELEMENT_MAX_NAME_BYTES,
                ),
                Some("checkbox") => {
                    was_capped(ffi::lv_checkbox_get_text(object), UI_ELEMENT_MAX_NAME_BYTES)
                }
                Some("button") => (0..ffi::lv_obj_get_child_count(object)
                    .min(UI_NAME_MAX_CHILDREN))
                    .find_map(|i| {
                        let child = ffi::lv_obj_get_child(object, i as i32);
                        ffi::lv_obj_has_class(child, &ffi::lv_label_class).then(|| {
                            was_capped(ffi::lv_label_get_text(child), UI_ELEMENT_MAX_NAME_BYTES)
                        })
                    })
                    .unwrap_or(false),
                _ => false,
            }
        };
    elements.push(UiElement {
        id,
        parent_id,
        role,
        semantic_role,
        name,
        text,
        hidden,
        disabled,
        focused,
        clickable,
        checked,
        bounds: (area.x1, area.y1, area.x2, area.y2),
        visible,
        clipped_bounds,
        layer,
        redacted,
        text_truncated,
        name_truncated,
        state: unsafe { ffi::lv_obj_get_state(object) },
        scroll: unsafe {
            (
                ffi::lv_obj_get_scroll_x(object),
                ffi::lv_obj_get_scroll_y(object),
            )
        },
        value,
        range,
        selected,
        selected_text,
        selected_text_truncated,
        style: unsafe { style(object) },
        buttons,
        buttons_truncated,
    });

    let child_count = unsafe { ffi::lv_obj_get_child_count(object) };
    if child_count == 0 {
        return;
    }
    if depth + 1 >= max_depth || elements.len() >= max_elements {
        *truncated = true;
        return;
    }
    for index in 0..child_count {
        if elements.len() >= max_elements {
            *truncated = true;
            break;
        }
        let child = unsafe { ffi::lv_obj_get_child(object, index as i32) };
        unsafe {
            inspect_children(
                child,
                Some(id),
                depth + 1,
                limits,
                elements,
                truncated,
                layer,
            );
        }
    }
}

/// Copy a valid C string with a hard byte cap without scanning past its NUL.
unsafe fn bounded_c_string(pointer: *const core::ffi::c_char) -> Option<String> {
    unsafe { bounded_c_string_with_limit(pointer, UI_ELEMENT_MAX_TEXT_BYTES) }
}

unsafe fn bounded_c_string_with_limit(
    pointer: *const core::ffi::c_char,
    limit: usize,
) -> Option<String> {
    if pointer.is_null() {
        return None;
    }

    let bytes = pointer.cast::<u8>();
    let mut length = 0;
    while length < limit && unsafe { *bytes.add(length) } != 0 {
        length += 1;
    }

    let slice = unsafe { core::slice::from_raw_parts(bytes, length) };
    Some(match str::from_utf8(slice) {
        Ok(text) => text.into(),
        Err(error) => String::from_utf8_lossy(&slice[..error.valid_up_to()]).into_owned(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiStyle {
    pub background: u32,
    pub text_color: u32,
    pub border_color: u32,
    pub opacity: u8,
    pub background_opacity: u8,
    pub border_width: i32,
    pub radius: i32,
    pub padding: (i32, i32, i32, i32),
    pub gap: (i32, i32),
    pub layout: u16,
    pub font_line_height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiButton {
    pub index: u32,
    pub text: Option<String>,
    pub text_truncated: bool,
    pub bounds: (i32, i32, i32, i32),
    pub hidden: bool,
    pub disabled: bool,
    pub checked: bool,
}

unsafe fn password_ancestor(mut obj: *mut ffi::lv_obj_t) -> bool {
    while !obj.is_null() {
        if unsafe {
            ffi::lv_obj_has_class(obj, &ffi::lv_textarea_class)
                && ffi::lv_textarea_get_password_mode(obj)
        } {
            return true;
        }
        obj = unsafe { ffi::lv_obj_get_parent(obj) };
    }
    false
}

/// Intersect with the display and clipping ancestors. Overflow-visible parents
/// do not clip children. This intentionally makes no occlusion claim.
unsafe fn clipped_bounds(obj: *mut ffi::lv_obj_t) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let mut area = ffi::lv_area_t::default();
        ffi::lv_obj_get_coords(obj, &mut area);
        let display = ffi::lv_obj_get_display(obj);
        area.x1 = area.x1.max(0);
        area.y1 = area.y1.max(0);
        area.x2 = area
            .x2
            .min(ffi::lv_display_get_horizontal_resolution(display) - 1);
        area.y2 = area
            .y2
            .min(ffi::lv_display_get_vertical_resolution(display) - 1);
        let mut parent = ffi::lv_obj_get_parent(obj);
        while !parent.is_null() {
            if !ffi::lv_obj_has_flag(parent, ffi::lv_obj_flag_t_LV_OBJ_FLAG_OVERFLOW_VISIBLE) {
                let mut p = ffi::lv_area_t::default();
                ffi::lv_obj_get_coords(parent, &mut p);
                area.x1 = area.x1.max(p.x1);
                area.y1 = area.y1.max(p.y1);
                area.x2 = area.x2.min(p.x2);
                area.y2 = area.y2.min(p.y2);
            }
            parent = ffi::lv_obj_get_parent(parent);
        }
        (area.x1 <= area.x2 && area.y1 <= area.y2).then_some((area.x1, area.y1, area.x2, area.y2))
    }
}

unsafe fn style(obj: *mut ffi::lv_obj_t) -> UiStyle {
    unsafe {
        UiStyle {
            background: ffi::lv_color_to_u32(ffi::lv_obj_get_style_bg_color(obj, 0)),
            text_color: ffi::lv_color_to_u32(ffi::lv_obj_get_style_text_color(obj, 0)),
            border_color: ffi::lv_color_to_u32(ffi::lv_obj_get_style_border_color(obj, 0)),
            opacity: ffi::lv_obj_get_style_opa_recursive(obj, 0),
            background_opacity: ffi::lv_obj_get_style_bg_opa(obj, 0),
            border_width: ffi::lv_obj_get_style_border_width(obj, 0),
            radius: ffi::lv_obj_get_style_radius(obj, 0),
            padding: (
                ffi::lv_obj_get_style_pad_top(obj, 0),
                ffi::lv_obj_get_style_pad_right(obj, 0),
                ffi::lv_obj_get_style_pad_bottom(obj, 0),
                ffi::lv_obj_get_style_pad_left(obj, 0),
            ),
            gap: (
                ffi::lv_obj_get_style_pad_row(obj, 0),
                ffi::lv_obj_get_style_pad_column(obj, 0),
            ),
            layout: ffi::lv_obj_get_style_layout(obj, 0),
            font_line_height: ffi::lv_font_get_line_height(ffi::lv_obj_get_style_text_font(obj, 0)),
        }
    }
}

#[derive(Default)]
struct WidgetValue {
    value: Option<i32>,
    range: Option<(i32, i32)>,
    selected: Option<u32>,
    selected_text: Option<String>,
    selected_text_truncated: bool,
}
unsafe fn widget_value(obj: *mut ffi::lv_obj_t, role: &str) -> WidgetValue {
    unsafe {
        let mut data = WidgetValue::default();
        match role {
            "lv_slider" => {
                data.value = Some(ffi::lv_slider_get_value(obj));
                data.range = Some((
                    ffi::lv_slider_get_min_value(obj),
                    ffi::lv_slider_get_max_value(obj),
                ));
            }
            "lv_bar" => {
                data.value = Some(ffi::lv_bar_get_value(obj));
                data.range = Some((
                    ffi::lv_bar_get_min_value(obj),
                    ffi::lv_bar_get_max_value(obj),
                ));
            }
            "lv_arc" => {
                data.value = Some(ffi::lv_arc_get_value(obj));
                data.range = Some((
                    ffi::lv_arc_get_min_value(obj),
                    ffi::lv_arc_get_max_value(obj),
                ));
            }
            "lv_spinbox" => data.value = Some(ffi::lv_spinbox_get_value(obj)),
            "lv_dropdown" | "lv_roller" => {
                let mut buffer = [0 as core::ffi::c_char; UI_ELEMENT_MAX_TEXT_BYTES + 4];
                data.selected = Some(if role == "lv_dropdown" {
                    ffi::lv_dropdown_get_selected_str(
                        obj,
                        buffer.as_mut_ptr(),
                        buffer.len() as u32,
                    );
                    ffi::lv_dropdown_get_selected(obj)
                } else {
                    ffi::lv_roller_get_selected_str(obj, buffer.as_mut_ptr(), buffer.len() as u32);
                    ffi::lv_roller_get_selected(obj)
                });
                data.selected_text = bounded_c_string(buffer.as_ptr());
                data.selected_text_truncated =
                    was_capped(buffer.as_ptr(), UI_ELEMENT_MAX_TEXT_BYTES);
            }
            "lv_tabview" => data.selected = Some(ffi::lv_tabview_get_tab_active(obj)),
            "lv_buttonmatrix" | "lv_keyboard" => {
                let selected = ffi::lv_buttonmatrix_get_selected_button(obj);
                data.selected = (selected != ffi::LV_BUTTONMATRIX_BUTTON_NONE).then_some(selected);
            }
            _ => {}
        }
        data
    }
}

unsafe fn matrix_buttons(
    obj: *mut ffi::lv_obj_t,
    role: &str,
    remaining: usize,
) -> (Vec<UiButton>, bool) {
    if !matches!(role, "lv_buttonmatrix" | "lv_keyboard") {
        return (Vec::new(), false);
    }
    // The pinned LVGL exposes this private layout; access stays within Core.
    unsafe {
        let matrix = &*obj.cast::<ffi::lv_buttonmatrix_t>();
        let count = matrix.btn_cnt.min(64).min(remaining as u32);
        if count == 0 {
            return (Vec::new(), matrix.btn_cnt > 0);
        }
        if matrix.map_p.is_null() || matrix.button_areas.is_null() || matrix.ctrl_bits.is_null() {
            return (Vec::new(), true);
        }

        let mut buttons = Vec::with_capacity(count as usize);
        let mut area = ffi::lv_area_t::default();
        ffi::lv_obj_get_coords(obj, &mut area);
        for index in 0..count {
            let text =
                bounded_c_string_with_limit(ffi::lv_buttonmatrix_get_button_text(obj, index), 128);
            let b = *matrix.button_areas.add(index as usize);
            let flags = *matrix.ctrl_bits.add(index as usize);
            buttons.push(UiButton {
                index,
                text_truncated: was_capped(ffi::lv_buttonmatrix_get_button_text(obj, index), 128),
                text,
                bounds: (
                    area.x1 + b.x1,
                    area.y1 + b.y1,
                    area.x1 + b.x2,
                    area.y1 + b.y2,
                ),
                hidden: flags & ffi::lv_buttonmatrix_ctrl_t_LV_BUTTONMATRIX_CTRL_HIDDEN != 0,
                disabled: flags & ffi::lv_buttonmatrix_ctrl_t_LV_BUTTONMATRIX_CTRL_DISABLED != 0,
                checked: flags & ffi::lv_buttonmatrix_ctrl_t_LV_BUTTONMATRIX_CTRL_CHECKED != 0,
            });
        }
        (buttons, matrix.btn_cnt > count)
    }
}

/// Return the pointer for an index in exactly the same bounded traversal. Used
/// only under the graphics lock within a single inspect-and-act operation.
pub(crate) unsafe fn object_at(
    root: *mut ffi::lv_obj_t,
    index: &mut u32,
    depth: usize,
) -> Option<*mut ffi::lv_obj_t> {
    if depth >= crate::UI_TREE_MAX_DEPTH {
        return None;
    }
    if *index == 0 {
        return Some(root);
    }
    *index -= 1;
    unsafe {
        for child in 0..ffi::lv_obj_get_child_count(root) {
            if let Some(obj) =
                object_at(ffi::lv_obj_get_child(root, child as i32), index, depth + 1)
            {
                return Some(obj);
            }
        }
    }
    None
}

unsafe fn was_capped(pointer: *const core::ffi::c_char, limit: usize) -> bool {
    if pointer.is_null() {
        return false;
    }
    for index in 0..limit {
        if unsafe { *pointer.add(index) } == 0 {
            return false;
        }
    }
    // Conservative at the exact cap: never read beyond bounded input.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_with_unallocated_arrays_is_incomplete_not_dereferenced() {
        let mut matrix = ffi::lv_buttonmatrix_t {
            btn_cnt: 1,
            ..Default::default()
        };
        let (buttons, truncated) = unsafe {
            matrix_buttons(
                (&mut matrix as *mut ffi::lv_buttonmatrix_t).cast(),
                "lv_buttonmatrix",
                64,
            )
        };
        assert!(buttons.is_empty());
        assert!(truncated);
    }
}
