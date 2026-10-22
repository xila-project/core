use graphics_bindgen::{
    CallCatalog,
    registry::{AdapterPolicy, AdapterRule, AutoObjectScalar, GraphicsRegistry},
};

const SIGNATURES: &str = r#"
unsafe extern "C" {
    fn lv_obj_set_width(obj: *mut lv_obj_t, width: i32);
    fn lv_obj_get_width(obj: *const lv_obj_t) -> i32;
    fn lv_obj_set_grid_dsc_array(obj: *mut lv_obj_t, columns: *const i32, rows: *const i32);
    fn lv_label_set_text(obj: *mut lv_obj_t, text: *const i8);
    fn lv_obj_set_style_bg_image_src(obj: *mut lv_obj_t, image: *const c_void, selector: u32);
}
"#;

#[test]
fn registry_uses_one_supported_set_and_marks_automatic_and_custom_adapters() {
    let catalog = CallCatalog::parse(SIGNATURES).unwrap();
    let registry = GraphicsRegistry::new(
        &catalog,
        &[
            "object_set_width",
            "object_get_width",
            "object_set_grid_dsc_array",
            "label_set_text",
        ],
        &[
            AdapterRule::automatic("object_set_width", AutoObjectScalar::SetI32),
            AdapterRule::automatic("object_get_width", AutoObjectScalar::GetI32),
            AdapterRule::custom("object_set_grid_dsc_array", "dispatch_grid_tracks"),
        ],
    )
    .unwrap();

    assert_eq!(registry.calls().len(), 4);
    assert_eq!(
        registry.adapter_for("object_set_width"),
        Some(AdapterPolicy::AutoObjectScalar(AutoObjectScalar::SetI32))
    );
    assert_eq!(
        registry.adapter_for("object_get_width"),
        Some(AdapterPolicy::AutoObjectScalar(AutoObjectScalar::GetI32))
    );
    assert_eq!(
        registry.adapter_for("object_set_grid_dsc_array"),
        Some(AdapterPolicy::Custom("dispatch_grid_tracks"))
    );
    assert_eq!(
        registry.adapter_for("label_set_text"),
        Some(AdapterPolicy::Manual)
    );
}

#[test]
fn registry_never_authorizes_retained_pointer_calls_as_automatic() {
    let catalog = CallCatalog::parse(SIGNATURES).unwrap();
    assert!(
        GraphicsRegistry::new(
            &catalog,
            &["object_set_grid_dsc_array"],
            &[AdapterRule::automatic(
                "object_set_grid_dsc_array",
                AutoObjectScalar::SetI32,
            )],
        )
        .is_err()
    );
}

#[test]
fn registry_rejects_adapter_rules_for_unregistered_calls() {
    let catalog = CallCatalog::parse(SIGNATURES).unwrap();
    assert!(
        GraphicsRegistry::new(
            &catalog,
            &["object_set_width"],
            &[AdapterRule::custom(
                "object_get_width",
                "dispatch_get_width"
            )],
        )
        .is_err()
    );
}

#[test]
fn generated_inventory_distinguishes_supported_and_unsupported_discovered_calls() {
    let catalog = CallCatalog::parse(SIGNATURES).unwrap();
    let registry = GraphicsRegistry::new(
        &catalog,
        &["object_set_width", "object_set_grid_dsc_array"],
        &[
            AdapterRule::automatic("object_set_width", AutoObjectScalar::SetI32),
            AdapterRule::custom("object_set_grid_dsc_array", "dispatch_grid_tracks"),
        ],
    )
    .unwrap();
    let inventory = registry.inventory_json(&catalog);
    assert!(inventory.contains("\"name\":\"object_set_width\""));
    assert!(inventory.contains("\"host_adapter\":\"auto_object_scalar\""));
    assert!(inventory.contains("\"custom_handler\":null"));
    assert!(inventory.contains("\"name\":\"object_set_style_bg_image_src\""));
    assert!(inventory.contains("\"custom_handler\":\"dispatch_grid_tracks\""));
    assert!(inventory.contains("\"guest_wrapper\":false"));
    assert!(inventory.contains("No host adapter registered"));
}

#[test]
fn actual_widget_registry_exposes_only_guest_safe_helpers_and_names_custom_adapters() {
    let catalog = CallCatalog::parse(lvgl_rust_sys::_bindgen_raw_src()).unwrap();
    let supported = [
        "calendar_set_today",
        "calendar_set_highlights",
        "calendar_get_shown_date",
        "chart_create",
        "chart_set_type",
        "chart_set_point_count",
        "chart_set_axis_range",
        "chart_add_series_slot",
        "chart_set_values",
        "chart_get_value",
        "line_create",
        "line_set_points",
        "line_set_y_invert",
        "list_create",
        "list_add_text",
        "list_add_text_button",
        "list_set_button_text",
        "tileview_create",
        "tileview_add_tile",
        "tileview_set_tile",
        "tileview_get_tile_active",
        "object_set_theme_variant",
        "object_set_theme_error",
        "badge_create",
        "card_create",
    ];
    let rules = [
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
        AdapterRule::custom("object_set_theme_variant", "dispatch_theme_variant"),
        AdapterRule::custom("object_set_theme_error", "dispatch_theme_error"),
        AdapterRule::custom("badge_create", "dispatch_badge_create"),
        AdapterRule::custom("card_create", "dispatch_card_create"),
    ];
    let registry = GraphicsRegistry::new(&catalog, &supported, &rules).unwrap();
    let inventory = registry.inventory_json(&catalog);

    for name in supported {
        let marker = format!("\"name\":\"{name}\"");
        let entry = inventory
            .split(&marker)
            .nth(1)
            .unwrap_or_else(|| panic!("inventory missing {name}"))
            .split('}')
            .next()
            .unwrap();
        assert!(
            entry.contains("\"guest_wrapper\":true"),
            "{name} not callable"
        );
    }
    assert_eq!(
        registry.adapter_for("chart_add_series_slot"),
        Some(AdapterPolicy::Custom("dispatch_chart_add_series"))
    );
    assert_eq!(
        registry.adapter_for("line_set_points"),
        Some(AdapterPolicy::Custom("dispatch_line_set_points"))
    );
    assert_eq!(registry.adapter_for("chart_add_series"), None);
    let raw_series = inventory
        .split("{\"name\":\"chart_add_series\"")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    assert!(raw_series.contains("\"guest_wrapper\":false"));
    assert!(raw_series.contains("No host adapter registered"));
}
