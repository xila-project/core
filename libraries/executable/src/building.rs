extern crate std;

use alloc::format;
use alloc::string::{String, ToString};
use std::path::PathBuf;
use std::process::Command;

pub fn build_crate(name: &str) -> Result<PathBuf, String> {
    log::information!("Building executable crate {}", name);

    let output = Command::new("cargo")
        .arg("build")
        .arg("--profile=release-wasm")
        .arg("--target=wasm32-wasip1")
        .arg("-p")
        .arg(name)
        .arg("--message-format=json")
        .output()
        .map_err(|e| format!("Failed to start cargo build: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
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
