use super::{Result, process};
use std::{env, fs, path::PathBuf, process::Command};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn checked(command: &mut Command) -> Result<()> {
    if !command.status()?.success() {
        return Err(format!("command failed: {command:?}").into());
    }
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let fixtures = root.join("tools/profiling/tests");
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let target = env::current_dir()?.join(target);
    let scratch_path = env::temp_dir().join(format!("xila-heaptrack-{}", std::process::id()));
    fs::create_dir(&scratch_path)?;
    let scratch = Scratch(scratch_path);
    fs::create_dir(scratch.0.join("src"))?;
    fs::copy(
        fixtures.join("allocation_probe.rs"),
        scratch.0.join("src/main.rs"),
    )?;
    let shim = scratch.0.join("shim.so");
    checked(
        Command::new(env::var_os("CC").unwrap_or_else(|| "cc".into()))
            .args(["-shared", "-fPIC"])
            .arg(fixtures.join("heaptrack_shim.c"))
            .arg("-o")
            .arg(&shim),
    )?;
    for enabled in [false, true] {
        let quote = |path: PathBuf| -> Result<String> {
            Ok(miniserde::json::to_string(&super::text(&path)?))
        };
        let manifest = format!(
            "[package]\nname = \"xila_heaptrack_probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\ndrivers_std = {{ path = {}, features = {} }}\nmemory = {{ path = {} }}\ncritical-section = {{ version = \"1.2\", features = [\"std\"] }}\n",
            quote(root.join("drivers/std"))?,
            if enabled { "[\"heaptrack\"]" } else { "[]" },
            quote(root.join("subsystems/memory"))?
        );
        fs::write(scratch.0.join("Cargo.toml"), manifest)?;
        checked(
            Command::new("cargo")
                .args(["build", "--manifest-path"])
                .arg(scratch.0.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target),
        )?;
        let events = scratch.0.join(format!("events-{enabled}.txt"));
        // Scope fixture-only preload variables to the probe process, never Cargo.
        // Use the same bounded process-group lifecycle as recordings.
        process::install_handlers()?;
        let args = vec![super::text(&target.join("debug/xila_heaptrack_probe"))?];
        let directory = scratch.0.join(format!("run-{enabled}"));
        fs::create_dir(&directory)?;
        let run = process::record(
            &args,
            &directory,
            10.,
            None,
            None,
            &[("LD_PRELOAD", &shim), ("XILA_TEST_HEAP_EVENTS", &events)],
        )?;
        if run.exit_code != 0 || run.termination != "exited" {
            return Err(format!(
                "allocation probe failed: {}",
                fs::read_to_string(directory.join("stderr.log"))?
            )
            .into());
        }
        let data = match fs::read_to_string(&events) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        let lines: Vec<Vec<&str>> = data
            .lines()
            .map(|line| line.split_whitespace().collect())
            .collect();
        let valid = if enabled {
            lines.len() == 4
                && lines[0].len() == 3
                && lines[1].len() == 3
                && lines[2].len() == 2
                && lines[3].len() == 2
                && lines[0][0] == "alloc"
                && lines[1][0] == "alloc"
                && lines[2][0] == "free"
                && lines[3][0] == "free"
                && lines[0][2] == "37"
                && lines[1][2] == "79"
                && lines[0][1] == lines[2][1]
                && lines[1][1] == lines[3][1]
        } else {
            lines.is_empty()
        };
        if !valid {
            return Err(format!("unexpected allocator events: {data}").into());
        }
    }
    println!(
        "Heaptrack smoke passed: enabled events, default silence, failed request, alignment and realloc preservation"
    );
    Ok(())
}
