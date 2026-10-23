#[path = "build/cache.rs"]
mod cache;
#[path = "build/fonts.rs"]
mod fonts;
#[path = "build/generate.rs"]
mod generate;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build");
    for variable in [
        "EMBEDDED_I18N_LOCALE",
        "EMBEDDED_I18N_FALLBACK",
        "PATH",
        "NODE_OPTIONS",
        "NODE_PATH",
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
    let mut variants = fonts::supported_variants();
    if !variants.contains(&active) {
        variants.push(active.clone());
    }
    let converter = root.join("../../node_modules/lv_font_conv/lv_font_conv.js");
    assert!(
        converter.is_file(),
        "Font converter not found at {}. Install workspace dependencies with `npm ci`.",
        converter.display()
    );
    let node =
        generate::node_path(&env::var_os("PATH").unwrap_or_default()).expect("Node.js executable");
    let (identity, inputs) = generate::common_inputs(&root, &node).expect("font generation inputs");
    for path in inputs {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let mut sources = Vec::new();
    let mut generated = 0;
    let mut reused = 0;
    for ranges in &variants {
        for source in generate::sources(&root, &out.join("generated_fonts"), ranges, &identity)
            .expect("font source plan")
        {
            if generate::generate(&source, &node, &converter)
                .unwrap_or_else(|error| panic!("Could not generate {}: {error}", source.name))
            {
                generated += 1;
            } else {
                reused += 1;
            }
            for path in &source.inputs {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            println!("cargo:rerun-if-changed={}", source.path.display());
            println!(
                "cargo:rerun-if-changed={}",
                source.path.with_extension("c.json").display()
            );
            sources.push(source.path);
        }
    }
    println!("cargo:warning=Font sources: generated {generated}, reused {reused}");
    // Only src/fonts.rs chooses which of these symbols Rust references.
    println!(
        "cargo:rustc-env=XILA_FONT_VARIANT={}",
        generate::variant_id(&active)
    );
    compile_font_archive(&out, &sources);
}

fn compile_font_archive(out: &Path, sources: &[PathBuf]) {
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
    // named ones. Always recreate the archive from the explicit current plan.
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
    build.files(sources).compile("font_assets");
}

fn required_metadata(name: &str) -> String {
    env::var(name)
        .unwrap_or_else(|_| panic!("Required {name} metadata is missing from lvgl_rust_sys build"))
}
