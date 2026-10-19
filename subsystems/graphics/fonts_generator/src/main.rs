use std::{env, fs, ops::Range, path::Path, process::Command};

use embedded_i18n::{format_ranges, merge_contiguous_ranges};

pub const FONTS: &[(u8, &str, &str, bool)] = &[
    (10, "Montserrat-Regular.ttf", "montserrat", true),
    (14, "Montserrat-Regular.ttf", "montserrat", true),
    (18, "Montserrat-Regular.ttf", "montserrat", true),
    (28, "Montserrat-Regular.ttf", "montserrat", true),
    (8, "unscii-16-full.woff", "unscii", false),
    (16, "unscii-16-full.woff", "unscii", false),
];
pub const BIT_PER_PIXEL: u8 = 4;
pub const EXTRA_RANGE: &[Range<u32>] = &[0xB0..0xB1, 0x2022..0x2023];
pub const FONT_AWESOME_RANGE: &[u32] = &[
    61441, 61448, 61451, 61452, 61452, 61453, 61457, 61459, 61461, 61465, 61468, 61473, 61478,
    61479, 61480, 61502, 61512, 61515, 61516, 61517, 61521, 61522, 61523, 61524, 61543, 61544,
    61550, 61552, 61553, 61556, 61559, 61560, 61561, 61563, 61587, 61589, 61636, 61637, 61639,
    61671, 61674, 61683, 61724, 61732, 61787, 61931, 62016, 62017, 62018, 62019, 62020, 62087,
    62099, 62212, 62189, 62810, 63231, 63426, 63650,
];
pub const FONTS_DIRECTORY: &str = "fonts";
pub const GENERATED_FONTS_DIRECTORY: &str = "generated_fonts";
pub const RANGES_FILE: &str = "ranges_lock.generated.c";

pub fn generate_font<'a>(
    size: u8,
    name: &str,
    font_path: impl AsRef<Path>,
    font_awesome_path: Option<impl AsRef<Path>>,
    output_path: impl AsRef<Path>,
    ranges: impl IntoIterator<Item = &'a Range<u32>>,
) -> Result<(), String> {
    let bits_per_pixel = BIT_PER_PIXEL.to_string();
    let size = size.to_string();
    let font_ranges = format_ranges(ranges);
    let font_awesome_range = FONT_AWESOME_RANGE
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<String>>()
        .join(",");

    let mut arguments = vec![
        "--yes",
        "github:lvgl/lv_font_conv",
        "--bpp",
        &bits_per_pixel,
        "--size",
        &size,
        "--no-compress",
        "--stride",
        "0",
        "--align",
        "1",
        "--font",
        font_path.as_ref().to_str().unwrap(),
        "-r",
        &font_ranges,
    ];

    if let Some(font_awesome_path) = font_awesome_path.as_ref() {
        arguments.extend(&[
            "--font",
            font_awesome_path.as_ref().to_str().unwrap(),
            "-r",
            &font_awesome_range,
        ]);
    }

    arguments.extend(&[
        "--format",
        "lvgl",
        "--lv-font-name",
        name,
        "-o",
        output_path.as_ref().to_str().unwrap(),
    ]);

    let command = Command::new("npx").args(arguments).spawn().map_err(|err| {
        format!(
            "Failed to execute lv_font_conv command. Is lv_font_conv installed and in your PATH? Error: {}",
            err
        )
    })?
    .wait()
    .map_err(|err| {
        format!(
            "Failed to wait for lv_font_conv command to finish. Error: {}",
            err
        )
    })?;

    if !command.success() {
        return Err("lv_font_conv command failed".to_string());
    }

    Ok(())
}

fn main() {
    let locale = embedded_i18n::get_locale_build();
    let fallback_locale = embedded_i18n::get_fallback_locale_build();

    let locale_range = embedded_i18n::get_locale_ranges(&locale).expect("Unsupported locale");
    let fallback_range =
        embedded_i18n::get_locale_ranges(&fallback_locale).expect("Unsupported fallback locale");

    let ranges = locale_range
        .iter()
        .cloned()
        .chain(fallback_range.iter().cloned())
        .chain(EXTRA_RANGE.iter().cloned())
        .collect::<Vec<Range<u32>>>();

    let ranges = merge_contiguous_ranges(ranges);

    let manifest_directory = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
        .canonicalize()
        .expect("Failed to canonicalize manifest directory path");

    println!(
        "Generating fonts for locale: {} (fallback: {}) in {}",
        locale,
        fallback_locale,
        file!()
    );

    let fonts_directory = manifest_directory
        .join(FONTS_DIRECTORY)
        .canonicalize()
        .expect("Failed to canonicalize fonts directory path");

    let generated_fonts_directory = manifest_directory.join(GENERATED_FONTS_DIRECTORY);

    // Create the generated_fonts directory if it doesn't exist
    fs::create_dir_all(&generated_fonts_directory)
        .expect("Failed to create generated fonts directory");

    let generated_fonts_directory = generated_fonts_directory
        .canonicalize()
        .expect("Failed to canonicalize generated fonts directory path");

    let ranges_string = format_ranges(ranges.iter());
    let ranges_file_path = generated_fonts_directory.join(RANGES_FILE);

    if fs::read_to_string(&ranges_file_path)
        .unwrap_or_default()
        .contains(&ranges_string)
    {
        println!("Font ranges unchanged, skipping font generation.");
        return;
    }

    for (size, path, name, enable_fontawesome) in FONTS.iter() {
        let font_path = fonts_directory.join(path);
        let font_awesome_path = if *enable_fontawesome {
            Some(fonts_directory.join("FontAwesome5-Solid+Brands+Regular.woff"))
            //Some(fonts_directory.join("FontAwesome7-Solid+Brands+Regular.otf"))
        } else {
            None
        };

        let name = format!("font_{name}_{size}");
        let output_path = generated_fonts_directory.join(format!("{name}.generated.c"));

        generate_font(
            *size,
            &name,
            font_path,
            font_awesome_path,
            output_path,
            &ranges,
        )
        .expect("Failed to generate font");
    }

    fs::write(&ranges_file_path, format!("// Ranges: {}\n", ranges_string))
        .expect("Failed to write ranges file");
}
