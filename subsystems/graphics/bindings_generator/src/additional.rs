use proc_macro2::TokenStream;
use quote::quote;

pub fn get() -> TokenStream {
    quote! {
        extern "C" {
            pub fn keyboard_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            pub fn keyboard_set_textarea(keyboard: *mut lv_obj_t, textarea: *mut lv_obj_t);
            pub fn object_set_padding(object: *mut lv_obj_t, padding: i32);
            /// Selects one of the generated Xila fonts by its pixel size.
            pub fn object_set_text_size(object: *mut lv_obj_t, size: u8);
            /// Creates the enabled LVGL switch widget.
            pub fn switch_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            /// Creates an enabled LVGL list widget.
            pub fn list_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            /// Adds a text row to a list and returns its label object.
            pub fn list_add_text(list: *mut lv_obj_t, text: *const core::ffi::c_char) -> *mut lv_obj_t;
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
