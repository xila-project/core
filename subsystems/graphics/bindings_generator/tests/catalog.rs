use graphics_bindgen::{CallCatalog, MAX_ARGUMENTS};
use quote::ToTokens;

#[test]
fn guest_state_constants_follow_lvgl_exports() {
    let generated =
        graphics_bindgen::guest_constants::generate(lvgl_rust_sys::_bindgen_raw_src()).unwrap();
    let file = syn::parse2::<syn::File>(generated).unwrap();
    let pressed = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(item) if item.ident == "STATE_PRESSED" => Some(item),
            _ => None,
        })
        .unwrap();
    let syn::Expr::Lit(value) = &*pressed.expr else {
        panic!("nonliteral state value")
    };
    let syn::Lit::Int(value) = &value.lit else {
        panic!("noninteger state value")
    };
    assert_eq!(
        value.base10_parse::<u32>().unwrap(),
        lvgl_rust_sys::lv_state_t_LV_STATE_PRESSED
    );
}

#[test]
fn ordered_calls_keep_unsupported_signatures_in_the_abi() {
    let catalog = CallCatalog::parse(
        r#"
        unsafe extern "C" {
            fn lv_obj_create(parent: *mut lv_obj_t) -> *mut lv_obj_t;
            fn lv_label_set_text_fmt(obj: *mut lv_obj_t, text: *const i8, ...);
            fn lv_label_set_text(obj: *mut lv_obj_t, text: *const i8);
            fn lv_obj_add_event_cb(obj: *mut lv_obj_t, cb: lv_event_cb_t);
            fn lv_timer_handler() -> u32;
        }
    "#,
    )
    .unwrap();
    let calls = catalog.calls();
    // The legacy filter includes the variadic call but excludes event callbacks.
    assert_eq!(calls[0].name, "label_set_text");
    assert_eq!(calls[0].id, 0);
    assert_eq!(calls[1].name, "label_set_text_fmt");
    assert_eq!(calls[1].id, 1);
    assert!(!calls[1].generic_safe());
    assert_eq!(calls[2].name, "object_create");
    assert_eq!(calls[2].id, 2);
    assert!(calls.iter().all(|call| !call.name.contains("event_cb")));
    let delete = calls
        .iter()
        .find(|call| call.name == "object_delete")
        .unwrap();
    assert_eq!(delete.argument_count, 1);
    assert_eq!(delete.host_arguments, ["__translator"]);
    assert_eq!(MAX_ARGUMENTS, 7);
}

#[test]
fn malformed_and_ambiguous_catalogs_have_diagnostics() {
    assert!(CallCatalog::parse("not Rust").is_err());
    let duplicate =
        CallCatalog::parse("unsafe extern \"C\" { fn lv_obj_create(); fn lv_object_create(); }");
    // Explicit same-normalized-name collision using a helper.
    let duplicate_helper =
        CallCatalog::parse("unsafe extern \"C\" { fn lv_obj_create(); fn lv_obj_create(); }");
    assert!(
        duplicate_helper
            .unwrap_err()
            .to_string()
            .contains("object_create")
    );
    assert!(duplicate.is_ok()); // lv_object_ isn't a legacy-authorized prefix.
    let too_many = CallCatalog::parse(
        "unsafe extern \"C\" { fn lv_obj_create(a:u32,b:u32,c:u32,d:u32,e:u32,f:u32,g:u32,h:u32); }",
    );
    assert!(too_many.unwrap_err().to_string().contains("seven"));
}

#[test]
fn retained_pointers_never_enter_the_generic_emitter() {
    let catalog = CallCatalog::parse(
        r#"unsafe extern "C" {
        fn lv_buttonmatrix_set_map(obj: *mut lv_obj_t, map: *const *const i8);
        fn lv_label_set_text_static(obj: *mut lv_obj_t, text: *const i8);
        fn lv_obj_set_width(obj: *mut lv_obj_t, width: i32);
        fn lv_point_transform(point: lv_point_t);
        fn lv_style_set_prop(value: lv_style_value_t);
    }"#,
    )
    .unwrap();
    assert!(
        !catalog
            .calls()
            .iter()
            .find(|c| c.name == "buttonmatrix_set_map")
            .unwrap()
            .generic_safe()
    );
    assert!(
        !catalog
            .calls()
            .iter()
            .find(|c| c.name == "label_set_text_static")
            .unwrap()
            .generic_safe()
    );
    assert!(
        catalog
            .calls()
            .iter()
            .find(|c| c.name == "object_set_width")
            .unwrap()
            .generic_safe()
    );
    assert!(
        !catalog
            .calls()
            .iter()
            .find(|c| c.name == "point_transform")
            .unwrap()
            .generic_safe()
    );
    assert!(
        !catalog
            .calls()
            .iter()
            .find(|c| c.name == "style_set_prop")
            .unwrap()
            .generic_safe()
    );
}

#[test]
fn actual_lvgl_catalog_matches_discovered_numbering() {
    let source = lvgl_rust_sys::_bindgen_raw_src();
    let catalog = CallCatalog::parse(source).unwrap();
    // Independently verify numbering against the currently exported LVGL API.
    let mut legacy = graphics_bindgen::context::LvglContext::default();
    use syn::visit::Visit;
    legacy.set_function_filtering(Some(
        graphics_bindgen::context::LvglContext::filter_function,
    ));
    legacy.visit_file(&syn::parse_file(source).unwrap());
    legacy.set_function_filtering(None);
    legacy.visit_file(&syn::parse2(graphics_bindgen::additional::get()).unwrap());
    let mut signatures = legacy.get_signatures();
    let extensions = [
        "indexed_canvas_create",
        "indexed_canvas_set_pixels",
        "window_pop_pointer_event",
        "keyboard_create",
        "keyboard_set_textarea",
        "object_set_padding",
        "grid_fr",
        "object_set_background",
        "object_set_text_color",
        "object_set_border",
        "object_set_radius",
        "object_set_text_align",
        "object_set_font",
        "object_set_surface",
        "object_set_min_max_size",
        "bar_create",
        "switch_create",
        "spinner_create",
        "range_set_range",
        "range_set_value",
        "range_get_value",
        "range_get_start_value",
        "range_set_mode",
        "range_set_orientation",
        "spinner_set_animation",
        "spinner_get_animation_duration",
        "spinner_get_arc_sweep",
        "dropdown_get_selected_string",
        "roller_get_selected_string",
        "dropdown_get_highlight",
        "dropdown_is_open_state",
    ];
    signatures.retain(|signature| !extensions.contains(&signature.ident.to_string().as_str()));
    signatures.sort_by_key(|s| s.ident.to_string().to_lowercase());
    let old_calls: Vec<_> = catalog
        .calls()
        .iter()
        .filter(|call| call.id < 0xf000)
        .collect();
    assert_eq!(signatures.len(), old_calls.len());
    for (id, (old, call)) in signatures.iter().zip(old_calls).enumerate() {
        assert_eq!(call.id as usize, id);
        assert_eq!(
            call.signature.to_token_stream().to_string(),
            old.to_token_stream().to_string()
        );
    }
    for (index, name) in extensions.iter().enumerate() {
        assert_eq!(
            catalog
                .calls()
                .iter()
                .find(|call| call.name == *name)
                .unwrap()
                .id,
            0xf000 + index as u16
        );
    }
    for name in [
        "window_create",
        "window_set_icon",
        "window_pop_event",
        "percentage",
        "object_create",
        "object_set_flex_flow",
        "object_set_flex_grow",
        "object_set_height",
        "object_set_width",
        "object_add_flag",
        "label_create",
        "label_set_text",
        "buttonmatrix_create",
        "buttonmatrix_set_map",
        "buttonmatrix_get_selected_button",
    ] {
        assert!(
            catalog.calls().iter().any(|call| call.name == name),
            "missing {name}"
        );
    }
}

#[test]
fn adapters_reject_changed_pointer_or_result_encodings() {
    let catalog = CallCatalog::parse(
        "unsafe extern \"C\" { fn lv_obj_create(parent: *mut lv_obj_t) -> *mut lv_obj_t; }",
    )
    .unwrap();
    let call = catalog
        .calls()
        .iter()
        .find(|call| call.name == "object_create")
        .unwrap();
    assert!(
        call.validate_signature(&["*mut lv_obj_t"], Some("*mut lv_obj_t"))
            .is_ok()
    );
    assert!(
        call.validate_signature(&["*const lv_obj_t"], Some("*mut lv_obj_t"))
            .is_err()
    );
    assert!(
        call.validate_signature(&["*mut lv_obj_t"], Some("u16"))
            .is_err()
    );
    assert!(call.validate_signature(&[], None).is_err());
}

#[test]
fn catalog_selection_keeps_original_ids_and_rejects_unknown_or_duplicate_names() {
    let catalog = CallCatalog::parse(
        r#"unsafe extern "C" {
            fn lv_obj_set_width(obj: *mut lv_obj_t, width: i32);
            fn lv_obj_get_width(obj: *const lv_obj_t) -> i32;
            fn lv_label_set_text(obj: *mut lv_obj_t, text: *const i8);
        }"#,
    )
    .unwrap();
    let width_id = catalog
        .calls()
        .iter()
        .find(|call| call.name == "object_set_width")
        .unwrap()
        .id;
    let selected = catalog
        .select(&["object_get_width", "object_set_width"])
        .unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>(),
        ["object_get_width", "object_set_width"]
    );
    assert_eq!(
        selected
            .iter()
            .find(|call| call.name == "object_set_width")
            .unwrap()
            .id,
        width_id
    );
    assert!(catalog.select(&["not_in_the_catalog"]).is_err());
    assert!(
        catalog
            .select(&["object_set_width", "object_set_width"])
            .is_err()
    );
}
