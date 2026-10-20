use std::fs;

pub const SHORTCUT_PATH: &str = "/configuration/shared/applications/shortcuts/synth.json";

#[unsafe(no_mangle)]
pub extern "C" fn __install() {
    fs::write(
        SHORTCUT_PATH,
        application_metadata::shortcut("synth").unwrap(),
    )
    .unwrap();
}
