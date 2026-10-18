//! Launch metadata shared by guest installers and reference-disk generation.
#![no_std]
extern crate alloc;
use alloc::{format, string::String};
embedded_i18n::track_locales!();

pub const GUESTS: &[&str] = &["calculator", "clock", "pixel_painter", "synth", "weather"];
pub const NATIVE: &[&str] = &["file_manager", "terminal", "settings"];
pub const PENDING_PATH: &str = "/system/.seed-pending";
pub const PENDING_CONTENT: &[u8] = b"xila-seed-v1\n";

pub fn shortcut(name: &str) -> Option<String> {
    let (label, icon, color, guest) = match name {
        "calculator" => ("Calculator", "Ca", [158, 158, 158], true),
        "clock" => ("Clock", "Cl", [59, 130, 246], true),
        "pixel_painter" => (
            embedded_i18n::translate!("Pixel Painter"),
            "Pa",
            [142, 36, 170],
            true,
        ),
        "synth" => ("Synth", "Sy", [103, 58, 183], true),
        "weather" => ("Weather", "We", [33, 150, 243], true),
        "file_manager" => ("File manager", "Fm", [0, 188, 212], false),
        "terminal" => ("Terminal", ">_", [0, 0, 0], false),
        "settings" => (
            embedded_i18n::translate!("Settings"),
            "Se",
            [158, 158, 158],
            false,
        ),
        _ => return None,
    };
    let command = if guest { "wasm" } else { name };
    let arguments = if guest {
        format!("\"/binaries/{name}\"")
    } else {
        String::new()
    };
    Some(format!(
        r#"{{"name": "{label}","command": "/binaries/{command}","arguments": [{arguments}],"terminal":false,"icon_string":"{icon}","icon_color":[{},{},{}]}}"#,
        color[0], color[1], color[2]
    ))
}
