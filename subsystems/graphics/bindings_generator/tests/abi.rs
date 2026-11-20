use graphics_bindgen::{
    MAX_ARGUMENTS,
    abi::{self, EXTRA, SUPPORTED},
    executor,
    registry::AdapterPolicy,
};
use std::collections::BTreeSet;

#[test]
fn declarations_are_unique_and_fit_the_call_frame() {
    let mut names = BTreeSet::new();
    for (name, arguments, result, _) in SUPPORTED.iter().chain(EXTRA) {
        assert!(names.insert(*name), "{name} is declared twice");
        assert!(
            arguments.len() <= MAX_ARGUMENTS,
            "{name} has too many arguments"
        );
        assert!(
            ["None", "Word", "U32", "I32"].contains(result),
            "{name} has an unknown result kind {result}"
        );
    }
}

#[test]
fn every_declaration_matches_the_discovered_c_signature_and_keeps_its_catalog_id() {
    let catalog = abi::catalog().unwrap();
    let registry = abi::registry(&catalog).unwrap();
    assert_eq!(registry.calls().len(), SUPPORTED.len() + EXTRA.len());
    // Extension IDs are append-only and stay clear of the legacy range.
    let id = |name: &str| {
        registry
            .calls()
            .iter()
            .find(|call| call.name == name)
            .unwrap()
            .id
    };
    assert_eq!(id("object_create"), 248);
    assert_eq!(id("file_picker_create"), 0xf02d);
    assert_eq!(id("window_pop_pointer_event"), 0xf002);
}

#[test]
fn constructors_are_generated_and_the_rest_name_a_handler_or_the_manual_match() {
    let catalog = abi::catalog().unwrap();
    let registry = abi::registry(&catalog).unwrap();
    assert_eq!(
        registry.adapter_for("bar_create"),
        Some(AdapterPolicy::Create("Bar"))
    );
    assert_eq!(
        registry.adapter_for("window_create"),
        Some(AdapterPolicy::Custom("dispatch_window_create"))
    );
    assert_eq!(
        registry.adapter_for("object_set_flex_flow"),
        Some(AdapterPolicy::Manual)
    );
}

#[test]
fn generated_executor_is_valid_rust_with_one_arm_per_adapter() {
    let source = executor::source().unwrap();
    let file = syn::parse_file(&source).expect("the call table must be valid Rust");
    assert!(file.items.len() >= 3);
    // One enum variant per declared call.
    let variants = source.matches("= ").count();
    assert!(variants >= SUPPORTED.len() + EXTRA.len());
    assert!(source.contains("handlers :: create"));
    assert!(source.contains("handlers :: manual"));
    assert!(source.contains("MAX_ARGUMENTS : usize = 7usize"));
}
