extern crate std;

use alloc::format;
use alloc::string::{String, ToString};
use std::path::{Path, PathBuf};

#[path = "building_command.rs"]
mod building_command;

fn build_failure_message(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let diagnostics = stdout
        .lines()
        .filter(|line| {
            line.contains("\"reason\":\"compiler-message\"") && line.contains("\"level\":\"error\"")
        })
        .collect::<alloc::vec::Vec<_>>()
        .join("\n");

    if diagnostics.is_empty() {
        format!("Cargo build failed.\n{stderr}\n{stdout}")
    } else {
        format!("Cargo build failed.\n{stderr}\n{diagnostics}")
    }
}

pub fn build_crate(name: &str) -> Result<PathBuf, String> {
    build_crate_with_manifest(name, None)
}

/// Build a guest from an explicitly selected Cargo workspace manifest.
pub fn build_crate_with_manifest(name: &str, manifest: Option<&Path>) -> Result<PathBuf, String> {
    log::information!("Building executable crate {}", name);
    let output = building_command::build_command(name, manifest)
        .output()
        .map_err(|e| format!("Failed to start cargo build: {}", e))?;

    if !output.status.success() {
        return Err(build_failure_message(&output.stdout, &output.stderr));
    }

    // Parse JSON messages to find the executable path
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut wasm_path = None;

    for line in stdout.lines() {
        // Look for compiler-artifact messages
        if line.contains("\"reason\":\"compiler-artifact\"") {
            // Check if this is a binary target
            if line.contains("\"kind\":[\"bin\"]") || line.contains("\"bin\"") {
                // Extract filenames array
                if let Some(filenames_start) = line.find("\"filenames\":[") {
                    let after_bracket = filenames_start + 13;
                    if let Some(filenames_end) = line[after_bracket..].find(']') {
                        let filenames_section = &line[after_bracket..after_bracket + filenames_end];
                        // Look for .wasm files in the filenames
                        for filename in filenames_section.split(',') {
                            let filename = filename.trim().trim_matches('"');
                            if filename.ends_with(".wasm") {
                                wasm_path = Some(filename.to_string());
                                break;
                            }
                        }
                        if wasm_path.is_some() {
                            break;
                        }
                    }
                }
            }
        }
    }

    let wasm_path = wasm_path.ok_or_else(|| {
        format!(
            "Could not find wasm executable in build output. Build output:\n{}",
            stdout
        )
    })?;

    log::information!("WASM executable built at {}", wasm_path);

    Ok(PathBuf::from(wasm_path))
}

#[cfg(test)]
mod tests {
    use super::{build_failure_message, std};
    use alloc::format;

    #[test]
    fn builds_a_guest_from_an_external_workspace_manifest() {
        use std::{
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "xila-external-guest-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"external-guest-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[profile.release-wasm]\ninherits = \"release\"\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"external-guest-fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let result = super::build_crate_with_manifest(
            "external-guest-fixture",
            Some(&root.join("Cargo.toml")),
        );
        match result {
            Ok(path) => {
                let wasm = fs::read(path).unwrap();
                assert!(wasm.starts_with(b"\0asm"));
            }
            Err(error) => panic!("external guest failed to build: {error}"),
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn build_failure_includes_json_compiler_diagnostics() {
        let stdout = br#"{"reason":"compiler-message","message":{"level":"error","message":"the useful compiler error"}}"#;
        let message = build_failure_message(stdout, b"error: could not compile dependency");

        assert!(message.contains("error: could not compile dependency"));
        assert!(message.contains("the useful compiler error"));
    }
}
