use std::fs;

pub const SHORTCUT: &str = r#"
{
    "name": "Synth",
    "command": "/binaries/wasm",
    "arguments": ["/binaries/synth"],
    "terminal": false,
    "icon_string": "Sy",
    "icon_color": [103, 58, 183]
}"#;

pub const SHORTCUT_PATH: &str = "/configuration/shared/shortcuts/synth.json";

#[unsafe(no_mangle)]
pub extern "C" fn __install() {
    fs::write(SHORTCUT_PATH, SHORTCUT).unwrap();
}
