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
    }
    #[cfg(feature = "graphics")]
    generate();
}

#[cfg(feature = "graphics")]
fn generate() {
    use graphics_bindgen::registry::{
        AdapterPolicy, AdapterRule, AutoObjectScalar, GraphicsRegistry,
    };
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
            "object_set_grid_dsc_array",
            &["*mut lv_obj_t", "*const i32", "*const i32"],
            "None",
            None,
        ),
        (
            "object_set_grid_align",
            &["*mut lv_obj_t", "lv_grid_align_t", "lv_grid_align_t"],
            "None",
            None,
        ),
        (
            "object_set_grid_cell",
            &[
                "*mut lv_obj_t",
                "lv_grid_align_t",
                "i32",
                "i32",
                "lv_grid_align_t",
                "i32",
                "i32",
            ],
            "None",
            None,
        ),
        ("grid_fr", &["u8"], "I32", Some("i32")),
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
        (
            "arc_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "slider_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "object_remove_state",
            &["*mut lv_obj_t", "lv_state_t"],
            "None",
            None,
        ),
        (
            "dropdown_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "dropdown_set_options",
            &["*mut lv_obj_t", "*const core::ffi::c_char"],
            "None",
            None,
        ),
        (
            "dropdown_set_selected",
            &["*mut lv_obj_t", "u32"],
            "None",
            None,
        ),
        (
            "dropdown_get_selected",
            &["*const lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "dropdown_get_option_count",
            &["*const lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "dropdown_set_selected_highlight",
            &["*mut lv_obj_t", "bool"],
            "None",
            None,
        ),
        ("dropdown_open", &["*mut lv_obj_t"], "None", None),
        ("dropdown_close", &["*mut lv_obj_t"], "None", None),
        (
            "roller_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "roller_set_options",
            &[
                "*mut lv_obj_t",
                "*const core::ffi::c_char",
                "lv_roller_mode_t",
            ],
            "None",
            None,
        ),
        (
            "roller_set_selected",
            &["*mut lv_obj_t", "u32", "lv_anim_enable_t"],
            "None",
            None,
        ),
        (
            "roller_get_selected",
            &["*const lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "roller_get_option_count",
            &["*const lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "roller_set_visible_row_count",
            &["*mut lv_obj_t", "u32"],
            "None",
            None,
        ),
        (
            "calendar_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "calendar_set_month_shown",
            &["*mut lv_obj_t", "u32", "u32"],
            "None",
            None,
        ),
        (
            "spinbox_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "spinbox_set_range",
            &["*mut lv_obj_t", "i32", "i32"],
            "None",
            None,
        ),
        ("spinbox_set_value", &["*mut lv_obj_t", "i32"], "None", None),
        ("spinbox_get_value", &["*mut lv_obj_t"], "I32", Some("i32")),
        ("spinbox_set_step", &["*mut lv_obj_t", "u32"], "None", None),
        (
            "spinbox_set_digit_format",
            &["*mut lv_obj_t", "u32", "u32"],
            "None",
            None,
        ),
        (
            "spinbox_set_rollover",
            &["*mut lv_obj_t", "bool"],
            "None",
            None,
        ),
        ("spinbox_increment", &["*mut lv_obj_t"], "None", None),
        ("spinbox_decrement", &["*mut lv_obj_t"], "None", None),
        (
            "chart_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "chart_set_type",
            &["*mut lv_obj_t", "lv_chart_type_t"],
            "None",
            None,
        ),
        (
            "chart_set_point_count",
            &["*mut lv_obj_t", "u32"],
            "None",
            None,
        ),
        (
            "chart_set_axis_range",
            &["*mut lv_obj_t", "lv_chart_axis_t", "i32", "i32"],
            "None",
            None,
        ),
        (
            "line_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "line_set_points",
            &["*mut lv_obj_t", "*const lv_point_precise_t", "u32"],
            "None",
            None,
        ),
        (
            "line_set_y_invert",
            &["*mut lv_obj_t", "bool"],
            "None",
            None,
        ),
        (
            "tileview_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "tileview_add_tile",
            &["*mut lv_obj_t", "u8", "u8", "lv_dir_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "tileview_set_tile",
            &["*mut lv_obj_t", "*mut lv_obj_t", "lv_anim_enable_t"],
            "None",
            None,
        ),
        (
            "tileview_get_tile_active",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
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
        (
            "object_set_background",
            &["*mut lv_obj_t", "u32", "u8", "u32"],
            "None",
            None,
        ),
        (
            "object_set_text_color",
            &["*mut lv_obj_t", "u32", "u32"],
            "None",
            None,
        ),
        (
            "object_set_border",
            &["*mut lv_obj_t", "u32", "i32", "u32", "u32"],
            "None",
            None,
        ),
        (
            "object_set_radius",
            &["*mut lv_obj_t", "i32", "u32"],
            "None",
            None,
        ),
        (
            "object_set_text_align",
            &["*mut lv_obj_t", "u32", "u32"],
            "None",
            None,
        ),
        (
            "object_set_font",
            &["*mut lv_obj_t", "u32", "u32"],
            "None",
            None,
        ),
        (
            "object_set_surface",
            &["*mut lv_obj_t", "u32"],
            "None",
            None,
        ),
        (
            "object_set_min_max_size",
            &["*mut lv_obj_t", "i32", "i32", "i32", "i32", "u32"],
            "None",
            None,
        ),
        (
            "bar_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "switch_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "spinner_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "led_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        ("led_set_brightness", &["*mut lv_obj_t", "u8"], "None", None),
        ("led_on", &["*mut lv_obj_t"], "None", None),
        ("led_off", &["*mut lv_obj_t"], "None", None),
        ("led_toggle", &["*mut lv_obj_t"], "None", None),
        (
            "range_set_range",
            &["*mut lv_obj_t", "i32", "i32"],
            "None",
            None,
        ),
        (
            "range_set_value",
            &["*mut lv_obj_t", "i32", "bool"],
            "None",
            None,
        ),
        ("range_get_value", &["*const lv_obj_t"], "I32", Some("i32")),
        (
            "range_get_start_value",
            &["*const lv_obj_t"],
            "I32",
            Some("i32"),
        ),
        ("range_set_mode", &["*mut lv_obj_t", "u32"], "None", None),
        (
            "range_set_orientation",
            &["*mut lv_obj_t", "u32"],
            "None",
            None,
        ),
        (
            "spinner_set_animation",
            &["*mut lv_obj_t", "u32", "u32"],
            "None",
            None,
        ),
        (
            "spinner_get_animation_duration",
            &["*const lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "spinner_get_arc_sweep",
            &["*const lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "dropdown_get_selected_string",
            &["*mut lv_obj_t", "*mut i8", "u32"],
            "I32",
            Some("i32"),
        ),
        (
            "roller_get_selected_string",
            &["*mut lv_obj_t", "*mut i8", "u32"],
            "I32",
            Some("i32"),
        ),
        (
            "dropdown_get_highlight",
            &["*mut lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "dropdown_is_open_state",
            &["*mut lv_obj_t"],
            "U32",
            Some("u32"),
        ),
        (
            "calendar_set_today",
            &["*mut lv_obj_t", "u32", "u32", "u32"],
            "None",
            None,
        ),
        (
            "calendar_set_highlights",
            &["*mut lv_obj_t", "*const u8", "usize"],
            "None",
            None,
        ),
        (
            "calendar_get_shown_date",
            &["*mut lv_obj_t", "*mut u8"],
            "None",
            None,
        ),
        (
            "chart_add_series_slot",
            &["*mut lv_obj_t", "u32"],
            "U32",
            Some("u32"),
        ),
        (
            "chart_set_values",
            &["*mut lv_obj_t", "u32", "*const i32", "usize"],
            "None",
            None,
        ),
        (
            "chart_get_value",
            &["*mut lv_obj_t", "u32", "u32"],
            "I32",
            Some("i32"),
        ),
        (
            "list_add_text_button",
            &["*mut lv_obj_t", "*const core::ffi::c_char"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "list_create",
            &["*mut lv_obj_t"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "list_add_text",
            &["*mut lv_obj_t", "*const core::ffi::c_char"],
            "Word",
            Some("*mut lv_obj_t"),
        ),
        (
            "list_set_button_text",
            &["*mut lv_obj_t", "*mut lv_obj_t", "*const core::ffi::c_char"],
            "None",
            None,
        ),
    ];
    let supported_names: Vec<_> = supported
        .iter()
        .chain(extra)
        .map(|(name, ..)| *name)
        .collect();
    let adapter_rules = [
        AdapterRule::automatic("object_set_width", AutoObjectScalar::SetI32),
        AdapterRule::automatic("object_set_height", AutoObjectScalar::SetI32),
        AdapterRule::automatic("object_set_pos", AutoObjectScalar::SetPairI32),
        AdapterRule::automatic("object_set_size", AutoObjectScalar::SetPairI32),
        AdapterRule::automatic("object_get_x", AutoObjectScalar::GetI32),
        AdapterRule::automatic("object_get_y", AutoObjectScalar::GetI32),
        AdapterRule::automatic("object_get_width", AutoObjectScalar::GetI32),
        AdapterRule::automatic("object_get_height", AutoObjectScalar::GetI32),
        AdapterRule::custom("object_set_grid_dsc_array", "dispatch_grid_tracks"),
        AdapterRule::custom("calendar_set_today", "dispatch_calendar_set_today"),
        AdapterRule::custom(
            "calendar_set_highlights",
            "dispatch_calendar_set_highlights",
        ),
        AdapterRule::custom(
            "calendar_get_shown_date",
            "dispatch_calendar_get_shown_date",
        ),
        AdapterRule::custom("chart_add_series_slot", "dispatch_chart_add_series"),
        AdapterRule::custom("chart_set_values", "dispatch_chart_set_values"),
        AdapterRule::custom("chart_get_value", "dispatch_chart_get_value"),
        AdapterRule::custom("line_set_points", "dispatch_line_set_points"),
        AdapterRule::custom("list_add_text", "dispatch_list_add_text"),
        AdapterRule::custom("list_add_text_button", "dispatch_list_add_text_button"),
        AdapterRule::custom("list_set_button_text", "dispatch_list_set_button_text"),
        AdapterRule::custom("tileview_add_tile", "dispatch_tileview_add_tile"),
        AdapterRule::custom("tileview_set_tile", "dispatch_tileview_set_tile"),
        AdapterRule::custom(
            "tileview_get_tile_active",
            "dispatch_tileview_get_tile_active",
        ),
    ];
    let registry = GraphicsRegistry::new(&catalog, &supported_names, &adapter_rules)
        .unwrap_or_else(|error| panic!("invalid graphics registry: {error}"));
    let output_path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        output_path.join("graphics_abi_inventory.json"),
        registry.inventory_json(&catalog),
    )
    .expect("failed to write graphics ABI inventory");
    #[cfg(feature = "guest")]
    guest::generate(&output_path, &registry);

    let mut variants = Vec::new();
    let mut lookup = Vec::new();
    let mut counts = Vec::new();
    let mut results = Vec::new();
    for call in registry.calls() {
        let (_, arguments, result, output) = supported
            .iter()
            .chain(extra)
            .find(|entry| entry.0 == call.name)
            .unwrap_or_else(|| panic!("registry call {} has no ABI declaration", call.name));
        call.validate_signature(arguments, *output)
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
    let mut dispatch_arms = Vec::new();
    for call in registry.calls() {
        let Some(policy) = registry.adapter_for(&call.name) else {
            continue;
        };
        let variant = graphics_bindgen::enumeration::get_variant_identifier(&call.signature.ident);
        let native = &call.signature.ident;
        match policy {
            AdapterPolicy::AutoObjectScalar(AutoObjectScalar::SetI32) => {
                dispatch_arms.push(quote! {
                    Function::#variant => {
                        let object = state.object(call.arguments[0], None)?;
                        let value = dispatch::signed_word(call.arguments[1])?;
                        unsafe { xila::graphics::lvgl::#native(object, value) };
                        Ok(true)
                    }
                });
            }
            AdapterPolicy::AutoObjectScalar(AutoObjectScalar::SetPairI32) => {
                dispatch_arms.push(quote! {
                    Function::#variant => {
                        let object = state.object(call.arguments[0], None)?;
                        let first = dispatch::signed_word(call.arguments[1])?;
                        let second = dispatch::signed_word(call.arguments[2])?;
                        unsafe { xila::graphics::lvgl::#native(object, first, second) };
                        Ok(true)
                    }
                });
            }
            AdapterPolicy::AutoObjectScalar(AutoObjectScalar::GetI32) => {
                dispatch_arms.push(quote! {
                    Function::#variant => {
                        let object = state.object(call.arguments[0], None)?;
                        let value = unsafe { xila::graphics::lvgl::#native(object) };
                        memory.write(
                            crate::host::translation::GuestPointer::<i32>::new(call.result),
                            value,
                        ).expect("validated result");
                        Ok(true)
                    }
                });
            }
            AdapterPolicy::Custom(handler) => {
                let handler = format_ident!("{handler}");
                dispatch_arms.push(quote! {
                    Function::#variant => {
                        dispatch::#handler(state, memory, call)?;
                        Ok(true)
                    }
                });
            }
            AdapterPolicy::Manual => continue,
        }
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
        pub(crate) fn dispatch_generated(
            state: &mut GraphicsState,
            memory: &mut crate::host::translation::GuestMemory<'_>,
            call: &dispatch::GraphicsCall,
            function: Function,
        ) -> crate::host::graphics::error::Result<bool> {
            match function {
                #(#dispatch_arms,)*
                _ => Ok(false),
            }
        }
    };
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("graphics.rs"),
        code.to_string(),
    )
    .unwrap();
}
