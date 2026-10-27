extern crate std;

use std::{path::Path, process::Command};

pub(crate) fn build_command(name: &str, manifest: Option<&Path>) -> Command {
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--locked")
        .arg("--profile=release-wasm")
        .arg("--target=wasm32-wasip1")
        .arg("-p")
        .arg(name)
        .arg("--message-format=json");
    if let Some(manifest) = manifest {
        command.arg("--manifest-path").arg(manifest);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::{build_command, std};
    use alloc::vec::Vec;
    use std::path::Path;

    #[test]
    fn external_workspace_manifest_is_passed_to_cargo() {
        let command = build_command("xila-weather", Some(Path::new("/apps/Cargo.toml")));
        let args: Vec<_> = command
            .get_args()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--manifest-path", "/apps/Cargo.toml"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-p", "xila-weather"]));
        assert!(args.iter().any(|arg| arg == "--target=wasm32-wasip1"));
        assert!(args.iter().any(|arg| arg == "--locked"));
    }
}
