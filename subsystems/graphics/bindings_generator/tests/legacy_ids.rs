//! Freeze the catalog built with the previous sys crate and Core's LVGL config.
//! New registry hooks must never renumber a released guest call.
use graphics_bindgen::CallCatalog;

#[test]
fn sys_migration_preserves_every_guest_call_id() {
    let catalog = CallCatalog::parse(lvgl_rust_sys::_bindgen_raw_src()).unwrap();
    let actual: String = catalog
        .calls()
        .iter()
        .map(|call| format!("{}\t{}\n", call.id, call.signature.ident))
        .collect();
    assert_eq!(actual, include_str!("fixtures/legacy_calls.tsv"));
}
