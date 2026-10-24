use std::{fs, path::Path};

#[test]
fn workspace_registers_synth_crate() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let manifest = fs::read_to_string(manifest).unwrap();

    assert!(
        manifest.contains("synth = { package = \"xila-synth\", path = \"executables/synth\" }")
    );
    assert!(manifest.contains("\"executables/synth\""));
}
