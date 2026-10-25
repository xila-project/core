//! Launch metadata shared by guest installers and reference-disk generation.
#![no_std]
extern crate alloc;
use alloc::{format, string::String};
embedded_i18n::track_locales!();

pub const GUESTS: &[&str] = &[
    "calculator",
    "clock",
    "pixel_painter",
    "synth",
    "text_editor",
    "weather",
];
#[cfg(unix)]
pub const NATIVE: &[&str] = &["file_manager", "terminal", "settings", "debug"];
#[cfg(not(unix))]
pub const NATIVE: &[&str] = &["file_manager", "terminal", "settings"];
pub const PENDING_PATH: &str = "/system/accounts/.seed-pending";
pub const PENDING_CONTENT: &[u8] = b"xila-seed-v1\n";

/// Stable compact identities used for application-private global storage.
pub fn application_identifier(name: &str) -> Option<task::ApplicationIdentifier> {
    let value = match name {
        "calculator" => 1,
        "clock" => 2,
        "pixel_painter" => 3,
        "synth" => 4,
        "text_editor" => 5,
        "weather" => 6,
        "file_manager" => 7,
        "terminal" => 8,
        "settings" => 9,
        "command_line_shell" => 10,
        "graphical_shell" => 11,
        _ => return None,
    };
    task::ApplicationIdentifier::new(value)
}

pub fn application_identifier_for_path(path: &str) -> Option<task::ApplicationIdentifier> {
    application_name_for_path(path).and_then(application_identifier)
}

pub fn application_name_for_path(path: &str) -> Option<&'static str> {
    if !path.starts_with("/applications/executables/") {
        return None;
    }
    let name = path.rsplit('/').next()?;
    let name = name.strip_suffix(".wasm").unwrap_or(name);
    [
        "calculator",
        "clock",
        "pixel_painter",
        "synth",
        "text_editor",
        "weather",
        "file_manager",
        "terminal",
        "settings",
        "command_line_shell",
        "graphical_shell",
    ]
    .into_iter()
    .find(|candidate| *candidate == name)
}

pub fn shortcut(name: &str) -> Option<String> {
    let (label, icon, color, guest) = match name {
        "calculator" => ("Calculator", "Ca", [158, 158, 158], true),
        "clock" => ("Clock", "Cl", [59, 130, 246], true),
        "pixel_painter" => (
            embedded_i18n::translate!("applications.pixel_painter"),
            "Pa",
            [142, 36, 170],
            true,
        ),
        "synth" => ("Synth", "Sy", [103, 58, 183], true),
        "text_editor" => (
            embedded_i18n::translate!("applications.text_editor"),
            "Te",
            [76, 110, 245],
            true,
        ),
        "weather" => ("Weather", "We", [33, 150, 243], true),
        "file_manager" => ("File manager", "Fm", [0, 188, 212], false),
        "terminal" => ("Terminal", ">_", [0, 0, 0], false),
        "settings" => (
            embedded_i18n::translate!("applications.settings"),
            "Se",
            [158, 158, 158],
            false,
        ),
        "debug" => ("Debug", "Db", [235, 87, 87], false),
        _ => return None,
    };
    let command = if guest { "wasm" } else { name };
    let arguments = if guest {
        format!("\"/applications/executables/{name}\"")
    } else {
        String::new()
    };
    Some(format!(
        r#"{{"name": "{label}","command": "/applications/executables/{command}","arguments": [{arguments}],"terminal":false,"icon_string":"{icon}","icon_color":[{},{},{}]}}"#,
        color[0], color[1], color[2]
    ))
}
