use std::{
    env, fs,
    ops::Range,
    path::{Path, PathBuf},
    process::Command,
};

use embedded_i18n::{format_ranges, merge_contiguous_ranges};

const FONTS: &[(u8, &str, &str, bool)] = &[
    (10, "Montserrat-Regular.ttf", "montserrat", true),
    (14, "Montserrat-Regular.ttf", "montserrat", true),
    (18, "Montserrat-Regular.ttf", "montserrat", true),
    (28, "Montserrat-Regular.ttf", "montserrat", true),
    (8, "unscii-16-full.woff", "unscii", false),
    (16, "unscii-16-full.woff", "unscii", false),
];
const BITS_PER_PIXEL: u8 = 4;
const EXTRA_RANGES: &[Range<u32>] = &[0xB0..0xB1, 0x2022..0x2023];
const FONT_AWESOME_RANGES: &[u32] = &[
    61441, 61448, 61451, 61452, 61452, 61453, 61457, 61459, 61461, 61465, 61468, 61473, 61478,
    61479, 61480, 61502, 61512, 61515, 61516, 61517, 61521, 61522, 61523, 61524, 61543, 61544,
    61550, 61552, 61553, 61556, 61559, 61560, 61561, 61563, 61587, 61589, 61636, 61637, 61639,
    61671, 61674, 61683, 61724, 61732, 61787, 61931, 62016, 62017, 62018, 62019, 62020, 62087,
    62099, 62212, 62189, 62810, 63231, 63426, 63650,
];
const FONT_AWESOME_FILE: &str = "FontAwesome5-Solid+Brands+Regular.woff";
const RANGE_MARKER: &str = "ranges_lock.generated.c";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=EMBEDDED_I18N_LOCALE");
    println!("cargo:rerun-if-env-changed=EMBEDDED_I18N_FALLBACK");
    println!("cargo:rerun-if-env-changed=DEP_LVGL_RUST_SYS_INCLUDE_PATHS");
    println!("cargo:rerun-if-env-changed=DEP_LVGL_RUST_SYS_DEFINES");
    println!("cargo:rerun-if-env-changed=DEP_LVGL_RUST_SYS_CFLAGS");
    if let Some(config_dir) = env::var_os("DEP_LV_CONFIG_PATH") {
        println!(
            "cargo:rerun-if-changed={}",
            PathBuf::from(config_dir).join("lv_conf.h").display()
        );
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let fonts_dir = manifest_dir.join("fonts");
    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let generated_dir = output_dir.join("generated_fonts");
    let converter = manifest_dir.join("../../node_modules/lv_font_conv/lv_font_conv.js");

    if !converter.is_file() {
        panic!(
            "Font converter not found at {}. Install workspace dependencies with `npm ci`.",
            converter.display()
        );
    }

    for (_, font_file, _, use_font_awesome) in FONTS {
        println!(
            "cargo:rerun-if-changed={}",
            fonts_dir.join(font_file).display()
        );
        if *use_font_awesome {
            println!(
                "cargo:rerun-if-changed={}",
                fonts_dir.join(FONT_AWESOME_FILE).display()
            );
        }
    }

    fs::create_dir_all(&generated_dir)
        .unwrap_or_else(|error| panic!("Failed to create {}: {error}", generated_dir.display()));

    let locale = embedded_i18n::get_locale_build();
    let fallback_locale = embedded_i18n::get_fallback_locale_build();
    let locale_ranges = embedded_i18n::get_locale_ranges(&locale)
        .unwrap_or_else(|| panic!("Unsupported font locale: {locale}"));
    let fallback_ranges = embedded_i18n::get_locale_ranges(&fallback_locale)
        .unwrap_or_else(|| panic!("Unsupported fallback font locale: {fallback_locale}"));
    let ranges = locale_ranges
        .iter()
        .cloned()
        .chain(fallback_ranges.iter().cloned())
        .chain(EXTRA_RANGES.iter().cloned())
        .collect::<Vec<_>>();
    let ranges = merge_contiguous_ranges(ranges);

    println!("Generating fonts for {locale} (fallback: {fallback_locale})");
    for (size, file_name, family, use_font_awesome) in FONTS {
        let name = format!("font_{family}_{size}");
        let font_path = fonts_dir.join(file_name);
        let font_awesome_path = use_font_awesome.then(|| fonts_dir.join(FONT_AWESOME_FILE));
        let output_path = generated_dir.join(format!("{name}.generated.c"));

        generate_font(
            &converter,
            *size,
            &name,
            &font_path,
            font_awesome_path.as_deref(),
            &output_path,
            &ranges,
        )
        .unwrap_or_else(|error| panic!("Failed to generate {name}: {error}"));
    }

    let ranges_string = format_ranges(ranges.iter());
    fs::write(
        generated_dir.join(RANGE_MARKER),
        format!("// Ranges: {ranges_string}\n"),
    )
    .unwrap_or_else(|error| panic!("Failed to write font range marker: {error}"));

    compile_font_archive(&generated_dir);
}

fn generate_font(
    converter: &Path,
    size: u8,
    name: &str,
    font_path: &Path,
    font_awesome_path: Option<&Path>,
    output_path: &Path,
    ranges: &[Range<u32>],
) -> Result<(), String> {
    let font_ranges = format_ranges(ranges.iter());
    let font_awesome_ranges = FONT_AWESOME_RANGES
        .iter()
        .map(|range| range.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut arguments = vec![
        "--bpp".to_string(),
        BITS_PER_PIXEL.to_string(),
        "--size".to_string(),
        size.to_string(),
        "--no-compress".to_string(),
        "--stride".to_string(),
        "0".to_string(),
        "--align".to_string(),
        "1".to_string(),
        "--font".to_string(),
        font_path.display().to_string(),
        "-r".to_string(),
        font_ranges,
    ];

    if let Some(font_awesome_path) = font_awesome_path {
        arguments.extend([
            "--font".to_string(),
            font_awesome_path.display().to_string(),
            "-r".to_string(),
            font_awesome_ranges,
        ]);
    }

    arguments.extend([
        "--format".to_string(),
        "lvgl".to_string(),
        "--lv-font-name".to_string(),
        name.to_string(),
        "-o".to_string(),
        output_path.display().to_string(),
    ]);

    let output = Command::new("node")
        .arg(converter)
        .args(arguments)
        .output()
        .map_err(|error| {
            format!("could not execute lv_font_conv through Node.js; install Node.js: {error}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "lv_font_conv exited with {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

fn compile_font_archive(generated_dir: &Path) {
    let include_paths = required_metadata("DEP_LVGL_RUST_SYS_INCLUDE_PATHS");
    let defines = required_metadata("DEP_LVGL_RUST_SYS_DEFINES");
    let cflags = env::var("DEP_LVGL_RUST_SYS_CFLAGS").unwrap_or_default();

    let mut build = cc::Build::new();
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

    let entries = fs::read_dir(generated_dir)
        .unwrap_or_else(|error| panic!("Failed to read {}: {error}", generated_dir.display()));
    for entry in entries {
        let source = entry
            .unwrap_or_else(|error| panic!("Failed to read generated font entry: {error}"))
            .path();
        if source.extension().is_some_and(|extension| extension == "c") {
            build.file(source);
        }
    }
    build.compile("font_assets");
}

fn required_metadata(name: &str) -> String {
    env::var(name)
        .unwrap_or_else(|_| panic!("Required {name} metadata is missing from lvgl_rust_sys build"))
}
