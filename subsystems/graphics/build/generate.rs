//! Build-only font generation; Rust consumers select symbols in src/fonts.rs.

use super::cache;
use embedded_i18n::format_ranges;
use std::{
    env,
    ffi::OsString,
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
    process::Command,
};

const FONTS: &[(u8, &str, &str, bool)] = &[
    (10, "Montserrat-Regular.ttf", "montserrat", true),
    (14, "Montserrat-Regular.ttf", "montserrat", true),
    (18, "Montserrat-Regular.ttf", "montserrat", true),
    (28, "Montserrat-Regular.ttf", "montserrat", true),
    (8, "unscii-16-full.woff", "unscii", false),
    (16, "unscii-16-full.woff", "unscii", false),
];
const ICON_FILE: &str = "FontAwesome5-Solid+Brands+Regular.woff";
const ICON_RANGES: &[u32] = &[
    61441, 61448, 61451, 61452, 61452, 61453, 61457, 61459, 61461, 61465, 61468, 61473, 61478,
    61479, 61480, 61502, 61512, 61515, 61516, 61517, 61521, 61522, 61523, 61524, 61543, 61544,
    61550, 61552, 61553, 61556, 61559, 61560, 61561, 61563, 61587, 61589, 61636, 61637, 61639,
    61671, 61674, 61683, 61724, 61732, 61787, 61931, 62016, 62017, 62018, 62019, 62020, 62087,
    62099, 62212, 62189, 62810, 63231, 63426, 63650,
];

pub fn variant_id(ranges: &[Range<u32>]) -> String {
    // Encode canonical half-open pairs directly: stable, collision-free C names.
    format!(
        "r{}",
        ranges
            .iter()
            .map(|r| format!("{:x}_{:x}", r.start, r.end))
            .collect::<Vec<_>>()
            .join("_")
    )
}

#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub path: PathBuf,
    pub identity: String,
    pub arguments: Vec<OsString>,
    pub inputs: Vec<PathBuf>,
}

pub fn sources(
    root: &Path,
    output: &Path,
    ranges: &[Range<u32>],
    common: &str,
) -> io::Result<Vec<Source>> {
    let variant = variant_id(ranges);
    FONTS
        .iter()
        .map(|(size, file, family, icons)| {
            let name = format!("font_{family}_{size}_{variant}");
            let font = root.join("fonts").join(file);
            let mut inputs = vec![font.clone()];
            let mut arguments = [
                "--bpp",
                "4",
                "--size",
                &size.to_string(),
                "--no-compress",
                "--stride",
                "0",
                "--align",
                "1",
                "--font",
            ]
            .map(OsString::from)
            .to_vec();
            arguments.extend([
                font.into_os_string(),
                "-r".into(),
                format_ranges(ranges).into(),
            ]);
            if *icons {
                let icon = root.join("fonts").join(ICON_FILE);
                inputs.push(icon.clone());
                arguments.extend([
                    "--font".into(),
                    icon.into_os_string(),
                    "-r".into(),
                    ICON_RANGES
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                        .into(),
                ]);
            }
            arguments.extend([
                "--format".into(),
                "lvgl".into(),
                "--lv-font-name".into(),
                name.clone().into(),
            ]);
            let input_hash = cache::input_digest(&inputs)?;
            let mut fields = vec![common.as_bytes(), input_hash.as_bytes()];
            fields.extend(arguments.iter().map(|arg| arg.as_encoded_bytes()));
            let identity = cache::digest(&fields);
            let path = output
                .join(&variant)
                .join(&identity)
                .join(format!("{name}.generated.c"));
            Ok(Source {
                name,
                path,
                identity,
                arguments,
                inputs,
            })
        })
        .collect()
}

pub fn node_path(paths: &std::ffi::OsStr) -> io::Result<PathBuf> {
    env::split_paths(paths)
        .map(|path| path.join(if cfg!(windows) { "node.exe" } else { "node" }))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            io::Error::other("Node.js not found in PATH; install Node.js and run npm ci")
        })
}

pub fn common_inputs(root: &Path, node: &Path) -> io::Result<(String, Vec<PathBuf>)> {
    let converter = root.join("../../node_modules/lv_font_conv");
    let mut inputs = cache::converter_inputs(&converter)?;
    inputs.extend(
        [
            "build.rs",
            "build/fonts.rs",
            "build/cache.rs",
            "build/generate.rs",
            "../../package-lock.json",
        ]
        .map(|path| root.join(path)),
    );
    inputs.push(node.to_path_buf());
    // Watch the PATH entry as well as its target when Node is a symlink.
    inputs.push(fs::canonicalize(node)?);
    let version = Command::new(node).arg("--version").output()?;
    if !version.status.success() {
        return Err(io::Error::other("Could not get Node.js version"));
    }
    let files = cache::input_digest(&inputs)?;
    let options = env::var_os("NODE_OPTIONS").unwrap_or_default();
    let search = env::var_os("NODE_PATH").unwrap_or_default();
    Ok((
        cache::digest(&[
            files.as_bytes(),
            &version.stdout,
            options.as_encoded_bytes(),
            search.as_encoded_bytes(),
        ]),
        inputs,
    ))
}

pub fn generate(source: &Source, node: &Path, converter: &Path) -> io::Result<bool> {
    cache::ensure_output(&source.path, &source.identity, |temporary| {
        let output = Command::new(node)
            .arg(converter)
            .args(&source.arguments)
            .arg("-o")
            .arg(temporary)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "lv_font_conv failed for {}: {}",
                source.name,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    })
}
