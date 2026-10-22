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
