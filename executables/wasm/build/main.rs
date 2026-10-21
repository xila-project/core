#[cfg(all(feature = "guest", feature = "graphics"))]
mod guest;
#[cfg(all(feature = "guest", feature = "graphics"))]
mod utilities;

fn main() {
    println!("cargo:rerun-if-changed=build/main.rs");
    #[cfg(all(feature = "guest", feature = "graphics"))]
    {
        println!("cargo:rerun-if-changed=build/guest.rs");
        println!("cargo:rerun-if-changed=build/utilities");
        println!("cargo:rerun-if-changed=src/guest/graphics/prelude.rs");
        println!("cargo:rerun-if-changed=src/graphics_event.rs");
        guest::generate(&std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").unwrap(),
        ));
    }
    #[cfg(feature = "graphics")]
    generate();
}

#[cfg(feature = "graphics")]
fn generate() {
    use quote::{format_ident, quote};
    let catalog = graphics_bindgen::CallCatalog::parse(lvgl_rust_sys::_bindgen_raw_src())
        .expect("invalid LVGL graphics ABI");
    // Every supported signature is explicit; all other catalog IDs remain reserved.
    type Adapter<'a> = (&'a str, &'a [&'a str], &'a str, Option<&'a str>);
    let supported: &[Adapter<'_>] = &[
        ("window_create", &[], "Word", Some("*mut lv_obj_t")),
        (
            "window_set_icon",
            &["*mut lv_obj_t", "*const core::ffi::c_char", "lv_color_t"],
            "None",
            None,
        ),
        (
            "window_pop_event",
            &["*mut lv_obj_t", "*mut u32", "*mut WasmPointer"],
            "None",
            None,
        ),
        ("percentage", &["i32"], "I32", Some("i32")),
        (
            "object_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        ("object_delete", &["u16"], "None", None),
        (
            "object_set_flex_flow",
            &["*mut lv_obj_t", "lv_flex_flow_t"],
            "None",
            None,
        ),
        (
            "object_set_flex_grow",
            &["*mut lv_obj_t", "u8"],
            "None",
            None,
        ),
        ("object_set_height", &["*mut lv_obj_t", "i32"], "None", None),
        ("object_set_width", &["*mut lv_obj_t", "i32"], "None", None),
        (
            "object_set_size",
            &["*mut lv_obj_t", "i32", "i32"],
            "None",
            None,
        ),
        (
            "object_set_pos",
            &["*mut lv_obj_t", "i32", "i32"],
            "None",
            None,
        ),
        ("object_get_x", &["*const lv_obj_t"], "I32", Some("i32")),
        ("object_get_y", &["*const lv_obj_t"], "I32", Some("i32")),
        ("object_get_width", &["*const lv_obj_t"], "I32", Some("i32")),
        (
            "object_get_height",
            &["*const lv_obj_t"],
            "I32",
            Some("i32"),
        ),
        (
            "object_align",
            &["*mut lv_obj_t", "lv_align_t", "i32", "i32"],
            "None",
            None,
        ),
        ("object_update_layout", &["*const lv_obj_t"], "None", None),
        (
            "object_set_flex_align",
            &[
                "*mut lv_obj_t",
                "lv_flex_align_t",
                "lv_flex_align_t",
                "lv_flex_align_t",
            ],
            "None",
            None,
        ),
        ("object_set_layout", &["*mut lv_obj_t", "u32"], "None", None),
        (
            "object_add_state",
            &["*mut lv_obj_t", "lv_state_t"],
            "None",
            None,
        ),
        (
            "object_get_state",
            &["*const lv_obj_t"],
            "U32",
            Some("lv_state_t"),
        ),
        (
            "button_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "checkbox_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "checkbox_set_text",
            &["*mut lv_obj_t", "*const core::ffi::c_char"],
            "None",
            None,
        ),
        (
            "tabview_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "tabview_add_tab",
            &["*mut lv_obj_t", "*const core::ffi::c_char"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "tabview_get_tab_active",
            &["*mut lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "object_add_flag",
            &["*mut lv_obj_t", "lv_obj_flag_t"],
            "None",
            None,
        ),
        (
            "label_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "label_set_text",
            &["*mut lv_obj_t", "*const core::ffi::c_char"],
            "None",
            None,
        ),
        (
            "buttonmatrix_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "buttonmatrix_set_map",
            &["*mut lv_obj_t", "*const *const core::ffi::c_char"],
            "None",
            None,
        ),
        (
            "buttonmatrix_get_selected_button",
            &["*const lv_obj_t"],
            "U32",
            Some("u32"),
        ),
    ];
    let extra: &[Adapter<'_>] = &[
        (
            "object_set_padding",
            &["*mut lv_obj_t", "i32"],
            "None",
            None,
        ),
        (
            "indexed_canvas_create",
            &["*mut lv_obj_t", "u16", "u16"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "indexed_canvas_set_pixels",
            &["*mut lv_obj_t", "*const u8", "usize", "*const u32", "usize"],
            "None",
            None,
        ),
        (
            "window_pop_pointer_event",
            &[
                "*mut lv_obj_t",
                "*mut u32",
                "*mut WasmPointer",
                "*mut i32",
                "*mut i32",
                "*mut i32",
                "*mut i32",
            ],
            "None",
            None,
        ),
        (
            "textarea_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "textarea_set_one_line",
            &["*mut lv_obj_t", "bool"],
            "None",
            None,
        ),
        (
            "textarea_set_text",
            &["*mut lv_obj_t", "*const core::ffi::c_char"],
            "None",
            None,
        ),
        (
            "textarea_set_max_length",
            &["*mut lv_obj_t", "u32"],
            "None",
            None,
        ),
        (
            "textarea_get_text",
            &["*mut lv_obj_t", "*mut i8", "usize"],
            "I32",
            Some("i32"),
        ),
        (
            "textarea_get_text_length",
            &["*mut lv_obj_t"],
            "I32",
            Some("i32"),
        ),
        (
            "keyboard_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "keyboard_set_textarea",
            &["*mut lv_obj_t", "*mut lv_obj_t"],
            "None",
            None,
        ),
        (
            "object_remove_flag",
            &["*mut lv_obj_t", "lv_obj_flag_t"],
            "None",
            None,
        ),
    ];
    let mut variants = Vec::new();
    let mut lookup = Vec::new();
    let mut counts = Vec::new();
    let mut results = Vec::new();
    for &(name, arguments, result, output) in supported.iter().chain(extra) {
        let call = catalog
            .calls()
            .iter()
            .find(|call| call.name == name)
            .unwrap_or_else(|| panic!("required LVGL call {name} missing"));
        call.validate_signature(arguments, output)
            .unwrap_or_else(|error| panic!("{error}"));
        let count = arguments.len() as i32;
        let variant = graphics_bindgen::enumeration::get_variant_identifier(&call.signature.ident);
        let id = call.id as i32;
        let result = format_ident!("{result}");
        variants.push(quote!(#variant = #id));
        lookup.push(quote!(#id => Some(Self::#variant)));
        counts.push(quote!(Self::#variant => #count));
        results.push(quote!(Self::#variant => ResultKind::#result));
    }
    let code = quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(i32)]
        pub enum Function { #(#variants,)* }
        impl Function {
            pub const fn id(self) -> i32 { self as i32 }
            pub fn from_id(id: i32) -> Option<Self> { match id { #(#lookup,)* _ => None } }
            pub(crate) fn argument_count(self) -> i32 { match self { #(#counts,)* } }
            pub(crate) fn result_kind(self) -> ResultKind { match self { #(#results,)* } }
        }
    };
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("graphics.rs"),
        code.to_string(),
    )
    .unwrap();
}
