//! The supported guest ABI: which catalog calls the executor implements, their
//! argument types and result kinds, and how each one is adapted.
//!
//! Core's build script generates the executor's call table from this list, and
//! guest bindings select the same calls, so both sides share one declaration.
//! Call IDs come from the catalog; the list never assigns them.

use crate::registry::{
    AdapterRule, AutoObjectScalar, GraphicsRegistry, KindTable, ObjectOperation, StyleValue,
};
use crate::{CallCatalog, GeneratorError};

/// Catalog name, argument types, result kind, and the native return type.
pub type Declaration = (
    &'static str,
    &'static [&'static str],
    &'static str,
    Option<&'static str>,
);

/// Calls that match the LVGL C API, in the order they were first exposed.
#[rustfmt::skip]
pub const SUPPORTED: &[Declaration] = &[
    ("window_create", &[], "Word", Some("*mut lv_obj_t")),
    ("file_picker_create", &["*mut lv_obj_t", "u32", "*const core::ffi::c_char"], "Word", Some("*mut lv_obj_t")),
    ("file_picker_reset", &["*mut lv_obj_t", "u32", "*const core::ffi::c_char"], "None", None),
    ("file_picker_get_status", &["*mut lv_obj_t"], "U32", Some("u32")),
    ("file_picker_get_path", &["*mut lv_obj_t", "*mut i8", "usize"], "I32", Some("i32")),
    ("window_set_icon", &["*mut lv_obj_t", "*const core::ffi::c_char", "lv_color_t"], "None", None),
    ("window_pop_event", &["*mut lv_obj_t", "*mut u32", "*mut WasmPointer"], "None", None),
    ("percentage", &["i32"], "I32", Some("i32")),
    ("object_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("object_delete", &["u32"], "None", None),
    ("object_set_flex_flow", &["*mut lv_obj_t", "lv_flex_flow_t"], "None", None),
    ("object_set_flex_grow", &["*mut lv_obj_t", "u8"], "None", None),
    ("object_set_height", &["*mut lv_obj_t", "i32"], "None", None),
    ("object_set_width", &["*mut lv_obj_t", "i32"], "None", None),
    ("object_set_size", &["*mut lv_obj_t", "i32", "i32"], "None", None),
    ("object_set_pos", &["*mut lv_obj_t", "i32", "i32"], "None", None),
    ("object_get_x", &["*const lv_obj_t"], "I32", Some("i32")),
    ("object_get_y", &["*const lv_obj_t"], "I32", Some("i32")),
    ("object_get_width", &["*const lv_obj_t"], "I32", Some("i32")),
    ("object_get_height", &["*const lv_obj_t"], "I32", Some("i32")),
    ("object_align", &["*mut lv_obj_t", "lv_align_t", "i32", "i32"], "None", None),
    ("object_update_layout", &["*const lv_obj_t"], "None", None),
    ("object_set_flex_align", &["*mut lv_obj_t", "lv_flex_align_t", "lv_flex_align_t", "lv_flex_align_t"], "None", None),
    ("object_set_layout", &["*mut lv_obj_t", "u32"], "None", None),
    ("object_set_grid_dsc_array", &["*mut lv_obj_t", "*const i32", "*const i32"], "None", None),
    ("object_set_grid_align", &["*mut lv_obj_t", "lv_grid_align_t", "lv_grid_align_t"], "None", None),
    ("object_set_grid_cell", &["*mut lv_obj_t", "lv_grid_align_t", "i32", "i32", "lv_grid_align_t", "i32", "i32"], "None", None),
    ("grid_fr", &["u8"], "I32", Some("i32")),
    ("object_add_state", &["*mut lv_obj_t", "lv_state_t"], "None", None),
    ("object_get_state", &["*const lv_obj_t"], "U32", Some("lv_state_t")),
    ("button_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("checkbox_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("checkbox_set_text", &["*mut lv_obj_t", "*const core::ffi::c_char"], "None", None),
    ("tabview_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("tabview_add_tab", &["*mut lv_obj_t", "*const core::ffi::c_char"], "Word", Some("*mut lv_obj_t")),
    ("tabview_get_tab_active", &["*mut lv_obj_t"], "U32", Some("u32")),
    ("object_add_flag", &["*mut lv_obj_t", "lv_obj_flag_t"], "None", None),
    ("label_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("label_set_text", &["*mut lv_obj_t", "*const core::ffi::c_char"], "None", None),
    ("buttonmatrix_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("buttonmatrix_set_map", &["*mut lv_obj_t", "*const *const core::ffi::c_char"], "None", None),
    ("buttonmatrix_get_selected_button", &["*const lv_obj_t"], "U32", Some("u32")),
    ("arc_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("slider_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("object_remove_state", &["*mut lv_obj_t", "lv_state_t"], "None", None),
    ("dropdown_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("dropdown_set_options", &["*mut lv_obj_t", "*const core::ffi::c_char"], "None", None),
    ("dropdown_set_selected", &["*mut lv_obj_t", "u32"], "None", None),
    ("dropdown_get_selected", &["*const lv_obj_t"], "U32", Some("u32")),
    ("dropdown_get_option_count", &["*const lv_obj_t"], "U32", Some("u32")),
    ("dropdown_set_selected_highlight", &["*mut lv_obj_t", "bool"], "None", None),
    ("dropdown_open", &["*mut lv_obj_t"], "None", None),
    ("dropdown_close", &["*mut lv_obj_t"], "None", None),
    ("roller_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("roller_set_options", &["*mut lv_obj_t", "*const core::ffi::c_char", "lv_roller_mode_t"], "None", None),
    ("roller_set_selected", &["*mut lv_obj_t", "u32", "lv_anim_enable_t"], "None", None),
    ("roller_get_selected", &["*const lv_obj_t"], "U32", Some("u32")),
    ("roller_get_option_count", &["*const lv_obj_t"], "U32", Some("u32")),
    ("roller_set_visible_row_count", &["*mut lv_obj_t", "u32"], "None", None),
    ("calendar_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("calendar_set_month_shown", &["*mut lv_obj_t", "u32", "u32"], "None", None),
    ("spinbox_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("spinbox_set_range", &["*mut lv_obj_t", "i32", "i32"], "None", None),
    ("spinbox_set_value", &["*mut lv_obj_t", "i32"], "None", None),
    ("spinbox_get_value", &["*mut lv_obj_t"], "I32", Some("i32")),
    ("spinbox_set_step", &["*mut lv_obj_t", "u32"], "None", None),
    ("spinbox_set_digit_format", &["*mut lv_obj_t", "u32", "u32"], "None", None),
    ("spinbox_set_rollover", &["*mut lv_obj_t", "bool"], "None", None),
    ("spinbox_increment", &["*mut lv_obj_t"], "None", None),
    ("spinbox_decrement", &["*mut lv_obj_t"], "None", None),
    ("chart_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("chart_set_type", &["*mut lv_obj_t", "lv_chart_type_t"], "None", None),
    ("chart_set_point_count", &["*mut lv_obj_t", "u32"], "None", None),
    ("chart_set_axis_range", &["*mut lv_obj_t", "lv_chart_axis_t", "i32", "i32"], "None", None),
    ("line_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("line_set_points", &["*mut lv_obj_t", "*const lv_point_precise_t", "u32"], "None", None),
    ("line_set_y_invert", &["*mut lv_obj_t", "bool"], "None", None),
    ("tileview_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("tileview_add_tile", &["*mut lv_obj_t", "u8", "u8", "lv_dir_t"], "Word", Some("*mut lv_obj_t")),
    ("tileview_set_tile", &["*mut lv_obj_t", "*mut lv_obj_t", "lv_anim_enable_t"], "None", None),
    ("tileview_get_tile_active", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
];

/// Xila helpers and widgets added after the first ABI, in append order.
#[rustfmt::skip]
pub const EXTRA: &[Declaration] = &[
    ("object_set_padding", &["*mut lv_obj_t", "i32"], "None", None),
    ("indexed_canvas_create", &["*mut lv_obj_t", "u16", "u16"], "Word", Some("*mut lv_obj_t")),
    ("indexed_canvas_set_pixels", &["*mut lv_obj_t", "*const u8", "usize", "*const u32", "usize"], "None", None),
    ("window_pop_pointer_event", &["*mut lv_obj_t", "*mut u32", "*mut WasmPointer", "*mut i32", "*mut i32", "*mut i32", "*mut i32"], "None", None),
    ("textarea_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("textarea_set_one_line", &["*mut lv_obj_t", "bool"], "None", None),
    ("textarea_set_text", &["*mut lv_obj_t", "*const core::ffi::c_char"], "None", None),
    ("textarea_set_max_length", &["*mut lv_obj_t", "u32"], "None", None),
    ("textarea_get_text", &["*mut lv_obj_t", "*mut i8", "usize"], "I32", Some("i32")),
    ("textarea_get_text_length", &["*mut lv_obj_t"], "I32", Some("i32")),
    ("keyboard_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("keyboard_set_textarea", &["*mut lv_obj_t", "*mut lv_obj_t"], "None", None),
    ("object_remove_flag", &["*mut lv_obj_t", "lv_obj_flag_t"], "None", None),
    ("object_set_background", &["*mut lv_obj_t", "u32", "u8", "u32"], "None", None),
    ("object_set_text_color", &["*mut lv_obj_t", "u32", "u32"], "None", None),
    ("object_set_border", &["*mut lv_obj_t", "u32", "i32", "u32", "u32"], "None", None),
    ("object_set_radius", &["*mut lv_obj_t", "i32", "u32"], "None", None),
    ("object_set_text_align", &["*mut lv_obj_t", "u32", "u32"], "None", None),
    ("object_set_font", &["*mut lv_obj_t", "u32", "u32"], "None", None),
    ("object_set_surface", &["*mut lv_obj_t", "u32"], "None", None),
    ("object_set_min_max_size", &["*mut lv_obj_t", "i32", "i32", "i32", "i32", "u32"], "None", None),
    ("bar_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("switch_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("spinner_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("led_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("led_set_brightness", &["*mut lv_obj_t", "u8"], "None", None),
    ("led_on", &["*mut lv_obj_t"], "None", None),
    ("led_off", &["*mut lv_obj_t"], "None", None),
    ("led_toggle", &["*mut lv_obj_t"], "None", None),
    ("range_set_range", &["*mut lv_obj_t", "i32", "i32"], "None", None),
    ("range_set_value", &["*mut lv_obj_t", "i32", "bool"], "None", None),
    ("range_get_value", &["*const lv_obj_t"], "I32", Some("i32")),
    ("range_get_start_value", &["*const lv_obj_t"], "I32", Some("i32")),
    ("range_set_mode", &["*mut lv_obj_t", "u32"], "None", None),
    ("range_set_orientation", &["*mut lv_obj_t", "u32"], "None", None),
    ("spinner_set_animation", &["*mut lv_obj_t", "u32", "u32"], "None", None),
    ("spinner_get_animation_duration", &["*const lv_obj_t"], "U32", Some("u32")),
    ("spinner_get_arc_sweep", &["*const lv_obj_t"], "U32", Some("u32")),
    ("dropdown_get_selected_string", &["*mut lv_obj_t", "*mut i8", "u32"], "I32", Some("i32")),
    ("roller_get_selected_string", &["*mut lv_obj_t", "*mut i8", "u32"], "I32", Some("i32")),
    ("dropdown_get_highlight", &["*mut lv_obj_t"], "U32", Some("u32")),
    ("dropdown_is_open_state", &["*mut lv_obj_t"], "U32", Some("u32")),
    ("calendar_set_today", &["*mut lv_obj_t", "u32", "u32", "u32"], "None", None),
    ("calendar_set_highlights", &["*mut lv_obj_t", "*const u8", "usize"], "None", None),
    ("calendar_get_shown_date", &["*mut lv_obj_t", "*mut u8"], "None", None),
    ("chart_add_series_slot", &["*mut lv_obj_t", "u32"], "U32", Some("u32")),
    ("chart_set_values", &["*mut lv_obj_t", "u32", "*const i32", "usize"], "None", None),
    ("chart_get_value", &["*mut lv_obj_t", "u32", "u32"], "I32", Some("i32")),
    ("list_add_text_button", &["*mut lv_obj_t", "*const core::ffi::c_char"], "Word", Some("*mut lv_obj_t")),
    ("list_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
    ("list_add_text", &["*mut lv_obj_t", "*const core::ffi::c_char"], "Word", Some("*mut lv_obj_t")),
    ("list_set_button_text", &["*mut lv_obj_t", "*mut lv_obj_t", "*const core::ffi::c_char"], "None", None),
    ("object_set_theme_variant", &["*mut lv_obj_t", "u32"], "None", None),
    ("object_set_theme_error", &["*mut lv_obj_t", "bool"], "None", None),
    ("badge_create", &["*mut lv_obj_t", "*const core::ffi::c_char", "u32"], "Word", Some("*mut lv_obj_t")),
    ("card_create", &["*mut lv_obj_t"], "Word", Some("*mut lv_obj_t")),
];

/// Constructors with the `(parent) -> object` shape. Each registers its new
/// object under the given kind; `window_create` has no parent.
const CREATE: &[(&str, &str)] = &[
    ("object_create", "Object"),
    ("label_create", "Label"),
    ("button_create", "Button"),
    ("checkbox_create", "Checkbox"),
    ("tabview_create", "TabView"),
    ("buttonmatrix_create", "ButtonMatrix"),
    ("arc_create", "Arc"),
    ("slider_create", "Slider"),
    ("dropdown_create", "Dropdown"),
    ("roller_create", "Roller"),
    ("calendar_create", "Calendar"),
    ("spinbox_create", "Spinbox"),
    ("chart_create", "Chart"),
    ("line_create", "Line"),
    ("tileview_create", "Tileview"),
    ("textarea_create", "Textarea"),
    ("keyboard_create", "Keyboard"),
    ("bar_create", "Bar"),
    ("switch_create", "Switch"),
    ("spinner_create", "Spinner"),
    ("led_create", "Led"),
    ("list_create", "List"),
];

use ObjectOperation::{
    Call, GetBool, GetI32, GetI32ByKind, GetU32, SetBool, SetEnum, SetEnumByKind, SetMask,
    SetRange, SetRangeByKind, SetStyle, SetText, SetU8, SetU32, SetU32Limit,
};
use StyleValue::{Color, Font};

/// Calls on one object: the object kind they require (`None` for any) and
/// what they do with their words, validated as the operation says. Calls whose
/// validation is specific to them stay in the handwritten match.
const OBJECT: &[(&str, Option<&str>, ObjectOperation)] = &[
    ("object_update_layout", None, Call),
    ("object_set_flex_grow", None, SetU8),
    ("object_set_layout", None, SetU32 { min: 0, max: 2 }),
    (
        "object_add_flag",
        None,
        SetU32 {
            min: 0,
            max: u32::MAX,
        },
    ),
    (
        "object_remove_flag",
        None,
        SetU32 {
            min: 0,
            max: u32::MAX,
        },
    ),
    ("object_add_state", None, SetMask { mask: 0xF3FD }),
    ("object_remove_state", None, SetMask { mask: 0xF3FD }),
    ("object_get_state", None, GetU32),
    ("led_on", Some("Led"), Call),
    ("led_off", Some("Led"), Call),
    ("led_toggle", Some("Led"), Call),
    ("led_set_brightness", Some("Led"), SetU8),
    ("spinbox_increment", Some("Spinbox"), Call),
    ("spinbox_decrement", Some("Spinbox"), Call),
    (
        "spinbox_set_step",
        Some("Spinbox"),
        SetU32 {
            min: 1,
            max: 1_000_000,
        },
    ),
    ("spinbox_set_rollover", Some("Spinbox"), SetBool),
    ("spinbox_get_value", Some("Spinbox"), GetI32),
    ("dropdown_open", Some("Dropdown"), Call),
    ("dropdown_close", Some("Dropdown"), Call),
    ("dropdown_set_selected_highlight", Some("Dropdown"), SetBool),
    ("dropdown_get_selected", Some("Dropdown"), GetU32),
    ("dropdown_get_option_count", Some("Dropdown"), GetU32),
    ("dropdown_get_highlight", Some("Dropdown"), GetBool),
    ("dropdown_is_open_state", Some("Dropdown"), GetBool),
    ("roller_get_selected", Some("Roller"), GetU32),
    ("roller_get_option_count", Some("Roller"), GetU32),
    (
        "roller_set_visible_row_count",
        Some("Roller"),
        SetU32 { min: 1, max: 32 },
    ),
    (
        "chart_set_point_count",
        Some("Chart"),
        SetU32 { min: 1, max: 128 },
    ),
    ("line_set_y_invert", Some("Line"), SetBool),
    ("textarea_set_one_line", Some("Textarea"), SetBool),
    ("spinner_get_animation_duration", Some("Spinner"), GetU32),
    ("spinner_get_arc_sweep", Some("Spinner"), GetU32),
    ("tabview_get_tab_active", Some("TabView"), GetU32),
    (
        "buttonmatrix_get_selected_button",
        Some("ButtonMatrix"),
        GetU32,
    ),
    (
        "chart_set_type",
        Some("Chart"),
        SetEnum {
            max: "lv_chart_type_t_LV_CHART_TYPE_SCATTER",
        },
    ),
    (
        "label_set_text",
        Some("Label"),
        SetText {
            limit: "MAX_STRING_BYTES",
        },
    ),
    (
        "checkbox_set_text",
        Some("Checkbox"),
        SetText {
            limit: "MAX_STRING_BYTES",
        },
    ),
    (
        "textarea_set_text",
        Some("Textarea"),
        SetText {
            limit: "MAX_TEXTAREA_BYTES",
        },
    ),
    (
        "textarea_set_max_length",
        Some("Textarea"),
        SetU32Limit { max: 4096 },
    ),
    ("spinbox_set_range", Some("Spinbox"), SetRange),
    ("object_set_text_color", None, SetStyle(Color)),
    (
        "object_set_radius",
        None,
        SetStyle(StyleValue::I32 { min: 0, max: 1024 }),
    ),
    (
        "object_set_text_align",
        None,
        SetStyle(StyleValue::Enum {
            max: "lv_text_align_t_LV_TEXT_ALIGN_RIGHT",
        }),
    ),
    ("object_set_font", None, SetStyle(Font)),
    ("range_get_value", None, GetI32ByKind(RANGE_VALUE)),
    ("range_get_start_value", None, GetI32ByKind(RANGE_START)),
    (
        "range_set_mode",
        None,
        SetEnumByKind {
            max: 2,
            table: RANGE_MODE,
        },
    ),
    (
        "range_set_orientation",
        None,
        SetEnumByKind {
            max: 2,
            table: RANGE_ORIENTATION,
        },
    ),
    ("range_set_range", None, SetRangeByKind(RANGE_RANGE)),
];

// The widgets behind the `range_*` helpers, each with its LVGL function.
const RANGE_VALUE: KindTable = &[
    ("Arc", "lv_arc_get_value"),
    ("Bar", "lv_bar_get_value"),
    ("Slider", "lv_slider_get_value"),
];
const RANGE_START: KindTable = &[
    ("Bar", "lv_bar_get_start_value"),
    ("Slider", "lv_slider_get_left_value"),
];
const RANGE_MODE: KindTable = &[
    ("Arc", "lv_arc_set_mode"),
    ("Bar", "lv_bar_set_mode"),
    ("Slider", "lv_slider_set_mode"),
];
const RANGE_ORIENTATION: KindTable = &[
    ("Bar", "lv_bar_set_orientation"),
    ("Slider", "lv_slider_set_orientation"),
    ("Switch", "lv_switch_set_orientation"),
];
const RANGE_RANGE: KindTable = &[
    ("Arc", "lv_arc_set_range"),
    ("Bar", "lv_bar_set_range"),
    ("Slider", "lv_slider_set_range"),
];

/// Xila helpers that wrap a differently named LVGL function.
const RENAMED: &[(&str, &str)] = &[
    (
        "spinner_get_animation_duration",
        "lv_spinner_get_anim_duration",
    ),
    (
        "dropdown_get_highlight",
        "lv_dropdown_get_selected_highlight",
    ),
    ("dropdown_is_open_state", "lv_dropdown_is_open"),
    ("object_set_text_color", "lv_obj_set_style_text_color"),
    ("object_set_radius", "lv_obj_set_style_radius"),
    ("object_set_text_align", "lv_obj_set_style_text_align"),
    ("object_set_font", "lv_obj_set_style_text_font"),
];

/// Per-call adapters. A call without a rule is dispatched by the executor's
/// handwritten match.
pub fn adapter_rules() -> Vec<AdapterRule> {
    let mut rules: Vec<AdapterRule> = [
        ("object_set_width", AutoObjectScalar::SetI32),
        ("object_set_height", AutoObjectScalar::SetI32),
        ("object_set_pos", AutoObjectScalar::SetPairI32),
        ("object_set_size", AutoObjectScalar::SetPairI32),
        ("object_get_x", AutoObjectScalar::GetI32),
        ("object_get_y", AutoObjectScalar::GetI32),
        ("object_get_width", AutoObjectScalar::GetI32),
        ("object_get_height", AutoObjectScalar::GetI32),
    ]
    .into_iter()
    .map(|(name, adapter)| AdapterRule::automatic(name, adapter))
    .collect();
    rules.extend(OBJECT.iter().map(|(name, kind, operation)| {
        let native = RENAMED
            .iter()
            .find(|(call, _)| call == name)
            .map(|(_, f)| *f);
        AdapterRule::object(name, *kind, *operation, native)
    }));
    rules.extend(
        CREATE
            .iter()
            .map(|(name, kind)| AdapterRule::create(name, kind)),
    );
    rules.extend(
        [
            ("object_set_grid_dsc_array", "dispatch_grid_tracks"),
            ("calendar_set_today", "dispatch_calendar_set_today"),
            (
                "calendar_set_highlights",
                "dispatch_calendar_set_highlights",
            ),
            (
                "calendar_get_shown_date",
                "dispatch_calendar_get_shown_date",
            ),
            ("chart_add_series_slot", "dispatch_chart_add_series"),
            ("chart_set_values", "dispatch_chart_set_values"),
            ("chart_get_value", "dispatch_chart_get_value"),
            ("line_set_points", "dispatch_line_set_points"),
            ("list_add_text", "dispatch_list_add_text"),
            ("list_add_text_button", "dispatch_list_add_text_button"),
            ("list_set_button_text", "dispatch_list_set_button_text"),
            ("tileview_add_tile", "dispatch_tileview_add_tile"),
            ("tileview_set_tile", "dispatch_tileview_set_tile"),
            (
                "tileview_get_tile_active",
                "dispatch_tileview_get_tile_active",
            ),
            ("object_set_theme_variant", "dispatch_theme_variant"),
            ("object_set_theme_error", "dispatch_theme_error"),
            ("badge_create", "dispatch_badge_create"),
            ("card_create", "dispatch_card_create"),
        ]
        .map(|(name, handler)| AdapterRule::custom(name, handler)),
    );
    rules
}

/// The declaration of a supported call, by catalog name.
pub fn declaration(name: &str) -> Option<&'static Declaration> {
    SUPPORTED
        .iter()
        .chain(EXTRA)
        .find(|declaration| declaration.0 == name)
}

/// The catalog of every call LVGL's bindings export, with stable IDs.
///
/// `bindings` is the Rust source `lvgl_rust_sys` generates for the build
/// target (its `BINDINGS` build metadata, or `_bindgen_raw_src()`).
pub fn catalog(bindings: &str) -> Result<CallCatalog, GeneratorError> {
    CallCatalog::parse(bindings)
}

/// Select every supported call from the catalog and check each declaration
/// against the discovered C signature.
pub fn registry(catalog: &CallCatalog) -> Result<GraphicsRegistry<'_>, GeneratorError> {
    let names: Vec<_> = SUPPORTED
        .iter()
        .chain(EXTRA)
        .map(|declaration| declaration.0)
        .collect();
    let registry = GraphicsRegistry::new(catalog, &names, &adapter_rules())?;
    for call in registry.calls() {
        let (_, arguments, _, output) = declaration(&call.name).expect("selected from the list");
        // `object_delete` takes a handle: u16 in the catalog, one word on the wire.
        if call.name == "object_delete" {
            call.validate_signature(&["u16"], *output)
                .or_else(|_| call.validate_signature(arguments, *output))?;
        } else {
            call.validate_signature(arguments, *output)?;
        }
    }
    Ok(registry)
}
