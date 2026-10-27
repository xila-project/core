//! Stable application identities and seed provisioning markers.
#![no_std]

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
        "activity" => 12,
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
        "activity",
    ]
    .into_iter()
    .find(|candidate| *candidate == name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn stable_activity_identifier_matches_its_executable_path() {
        assert_eq!(
            super::application_name_for_path("/applications/executables/activity"),
            Some("activity")
        );
        assert_eq!(
            super::application_identifier("activity"),
            super::application_identifier_for_path("/applications/executables/activity")
        );
        assert!(super::application_identifier("activity").is_some());
    }
}
