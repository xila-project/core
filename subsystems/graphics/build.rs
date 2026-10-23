#[path = "build/fonts.rs"]
mod fonts;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use embedded_i18n::format_ranges;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build");
    for variable in [
        "EMBEDDED_I18N_LOCALE",
        "EMBEDDED_I18N_FALLBACK",
        "PATH",
        "DEP_LV_CONFIG_PATH",
        "DEP_LVGL_RUST_SYS_INCLUDE_PATHS",
        "DEP_LVGL_RUST_SYS_DEFINES",
        "DEP_LVGL_RUST_SYS_CFLAGS",
        "DEP_LVGL_RUST_SYS_TARGET",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    if let Some(config_dir) = env::var_os("DEP_LV_CONFIG_PATH") {
        println!(
            "cargo:rerun-if-changed={}",
            PathBuf::from(config_dir).join("lv_conf.h").display()
        );
    }

    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let active = fonts::effective_ranges(
        &embedded_i18n::get_locale_build(),
        &embedded_i18n::get_fallback_locale_build(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let variants = fonts::supported_variants();
    let converter = root.join("../../node_modules/lv_font_conv/lv_font_conv.js");
    assert!(
        converter.is_file(),
        "Font converter not found at {}. Install workspace dependencies with `npm ci`.",
        converter.display()
    );
    let output = Command::new("node")
        .arg(root.join("build/generate.mjs"))
        .arg(&root)
        .arg(out.join("generated_fonts"))
        .arg(format_ranges(&active))
        .args(variants.iter().map(format_ranges))
        .output()
        .expect("Could not execute font generator; install Node.js and run npm ci");
    assert!(
        output.status.success(),
        "Font generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).expect("font generator UTF-8 output");
    let mut selected = Vec::new();
    for line in output.lines() {
        if line.starts_with("cargo:") {
            println!("{line}");
        } else if let Some(path) = line.strip_prefix("source\t") {
            selected.push(PathBuf::from(path));
        } else {
            panic!("Unexpected font generator output: {line}");
        }
    }
    assert_eq!(selected.len(), 6, "expected six selected font sources");
    compile_font_archive(&out, &selected);
}

fn compile_font_archive(out: &Path, selected: &[PathBuf]) {
    let include_paths = required_metadata("DEP_LVGL_RUST_SYS_INCLUDE_PATHS");
    let defines = required_metadata("DEP_LVGL_RUST_SYS_DEFINES");
    let cflags = required_metadata("DEP_LVGL_RUST_SYS_CFLAGS");
    let target = required_metadata("DEP_LVGL_RUST_SYS_TARGET");

    let mut build = cc::Build::new();
    build.target(&target).warnings(true);
    for include in include_paths.split(';').filter(|path| !path.is_empty()) {
        build.include(include);
    }
    for entry in defines
        .split(';')
        .chain(cflags.split(','))
        .filter(|entry| !entry.is_empty())
    {
        let mut parts = entry.splitn(2, '=');
        let name = parts.next().expect("non-empty define");
        build.define(name, parts.next());
    }
    // Archivers may replace named members without removing old, differently
    // named ones. Always start this build-owned archive empty on locale changes.
    let archive = out.join(if target.contains("msvc") {
        "font_assets.lib"
    } else {
        "libfont_assets.a"
    });
    match fs::remove_file(&archive) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("Could not replace {}: {error}", archive.display()),
    }
    build.files(selected).compile("font_assets");
}

fn required_metadata(name: &str) -> String {
    env::var(name)
        .unwrap_or_else(|_| panic!("Required {name} metadata is missing from lvgl_rust_sys build"))
}
