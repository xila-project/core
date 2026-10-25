use std::fs;
use std::path::Path;

#[test]
fn workspace_registers_weather_crate() {
    let workspace_manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let workspace_manifest = fs::read_to_string(workspace_manifest).unwrap();

    assert!(
        workspace_manifest
            .contains("weather = { package = \"xila-weather\", path = \"executables/weather\" }"),
        "workspace.dependencies must declare weather path dependency"
    );
    assert!(
        workspace_manifest.contains("\"executables/weather\""),
        "workspace members must include executables/weather"
    );
}
