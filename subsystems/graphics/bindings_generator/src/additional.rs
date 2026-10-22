use proc_macro2::TokenStream;
use quote::quote;

pub fn get() -> TokenStream {
    quote! {
        extern "C" {
            pub fn keyboard_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            pub fn keyboard_set_textarea(keyboard: *mut lv_obj_t, textarea: *mut lv_obj_t);
            pub fn object_set_padding(object: *mut lv_obj_t, padding: i32);
            /// Encodes a bounded fractional Grid track weight for a guest descriptor array.
            pub fn grid_fr(weight: u8) -> i32;
            /// Validated local styling without exposing native style or color pointers.
            pub fn object_set_background(object: *mut lv_obj_t, rgb: u32, opacity: u8, selector: u32);
            pub fn object_set_text_color(object: *mut lv_obj_t, rgb: u32, selector: u32);
            pub fn object_set_border(object: *mut lv_obj_t, rgb: u32, width: i32, side: u32, selector: u32);
            pub fn object_set_radius(object: *mut lv_obj_t, radius: i32, selector: u32);
            pub fn object_set_text_align(object: *mut lv_obj_t, align: u32, selector: u32);
            /// Font and semantic surface IDs are host-defined, not native pointers.
            pub fn object_set_font(object: *mut lv_obj_t, font: u32, selector: u32);
            pub fn object_set_surface(object: *mut lv_obj_t, surface: u32);
            pub fn object_set_min_max_size(object: *mut lv_obj_t, min_width: i32, max_width: i32, min_height: i32, max_height: i32, selector: u32);
            /// Shared bounded operations for LVGL range controls.
            pub fn bar_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            pub fn switch_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            pub fn spinner_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            pub fn range_set_range(object: *mut lv_obj_t, min: i32, max: i32);
            pub fn range_set_value(object: *mut lv_obj_t, value: i32, animated: bool);
            pub fn range_get_value(object: *const lv_obj_t) -> i32;
            pub fn range_get_start_value(object: *const lv_obj_t) -> i32;
            pub fn range_set_mode(object: *mut lv_obj_t, mode: u32);
            pub fn range_set_orientation(object: *mut lv_obj_t, orientation: u32);
            pub fn spinner_set_animation(object: *mut lv_obj_t, duration_ms: u32, sweep_degrees: u32);
            pub fn spinner_get_animation_duration(object: *const lv_obj_t) -> u32;
            pub fn spinner_get_arc_sweep(object: *const lv_obj_t) -> u32;
            /// Copies a selected option into a checked guest output span.
            pub fn dropdown_get_selected_string(object: *mut lv_obj_t, output: *mut i8, capacity: u32) -> i32;
            pub fn roller_get_selected_string(object: *mut lv_obj_t, output: *mut i8, capacity: u32) -> i32;
            /// Boolean results are widened to guest u32 values.
            pub fn dropdown_get_highlight(object: *mut lv_obj_t) -> u32;
            pub fn dropdown_is_open_state(object: *mut lv_obj_t) -> u32;
            /// Validated Calendar operations; date arrays and output records use packed guest ABI bytes.
            pub fn calendar_set_today(calendar: *mut lv_obj_t, year: u32, month: u32, day: u32);
            pub fn calendar_set_highlights(calendar: *mut lv_obj_t, dates: *const u8, count: usize);
            pub fn calendar_get_shown_date(calendar: *mut lv_obj_t, output: *mut u8);
            /// Chart series stay host-selected by slot; these helpers never expose LVGL series pointers.
            pub fn chart_add_series_slot(chart: *mut lv_obj_t, rgb: u32) -> u32;
            pub fn chart_set_values(chart: *mut lv_obj_t, series_slot: u32, values: *const i32, count: usize);
            pub fn chart_get_value(chart: *mut lv_obj_t, series_slot: u32, index: u32) -> i32;
            /// List helpers use checked object handles and copied UTF-8 strings.
            pub fn list_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            pub fn list_add_text(list: *mut lv_obj_t, text: *const core::ffi::c_char) -> *mut lv_obj_t;
            pub fn list_add_text_button(list: *mut lv_obj_t, text: *const core::ffi::c_char) -> *mut lv_obj_t;
            pub fn list_set_button_text(list: *mut lv_obj_t, button: *mut lv_obj_t, text: *const core::ffi::c_char);
            /// Host-owned indexed canvas. Dimensions and uploads are bounded by the host.
            pub fn indexed_canvas_create(parent: *mut lv_obj_t, width: u16, height: u16) -> *mut lv_obj_t;

            /// Copies all pixels and RGB888 palette entries; no guest pointers are retained.
            pub fn indexed_canvas_set_pixels(canvas: *mut lv_obj_t, pixels: *const u8, length: usize, palette: *const u32, colors: usize);

            /// Like window_pop_event, with event-time pointer coordinates and content size.
            /// Non-pointer events write i32::MIN to x/y and zero to width/height.
            pub fn window_pop_pointer_event(window: *mut lv_obj_t, code: *mut u32, target: *mut WasmPointer, x: *mut i32, y: *mut i32, width: *mut i32, height: *mut i32);
            pub fn object_delete(__translator : &mut Translator, object: u16);

            pub fn window_create() -> *mut lv_obj_t;

            pub fn window_pop_event(
                __translator: &mut Translator,
                window: *mut lv_obj_t,
                code: *mut u32,
                target: *mut WasmPointer
            );

            pub fn window_get_event_code(window: *mut lv_obj_t) -> u32;

            pub fn window_get_event_target(__translator: &mut Translator, window: *mut lv_obj_t) -> u16;

            pub fn window_next_event(window: *mut lv_obj_t);

            pub fn window_set_icon(
                window: *mut lv_obj_t,
                icon_string: *const core::ffi::c_char,
                icon_color: lv_color_t
            );

            pub fn percentage(value: i32) -> i32;

            pub fn textarea_get_text(textarea: *mut lv_obj_t, buffer: *mut i8, buffer_size: usize) -> i32;

            pub fn textarea_get_text_length(textarea: *mut lv_obj_t) -> i32;
        }
    }
}
