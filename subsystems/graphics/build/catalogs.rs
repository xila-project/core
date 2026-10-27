use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Default, Debug, Clone)]
pub struct CatalogSymbols {
    pub cjk: String,
    pub other: String,
    pub directories: Vec<PathBuf>,
}

/// Collect translated glyphs and directories Cargo must watch for catalog changes.
#[allow(dead_code)]
pub fn collect(root: &Path, locales: &[&str]) -> io::Result<CatalogSymbols> {
    collect_multiple(&[root], locales)
}

/// Combine catalogs from the Core checkout and explicitly supplied workspaces.
pub fn collect_multiple(roots: &[&Path], locales: &[&str]) -> io::Result<CatalogSymbols> {
    let mut expanded = BTreeSet::new();
    for locale in locales.iter().flat_map(|locales| locales.split(',')) {
        let locale = super::fonts::normalize_locale(locale);
        if locale.is_empty() {
            continue;
        }
        if let Some((base, _)) = locale.split_once('-') {
            expanded.insert(base.to_owned());
        }
        expanded.insert(locale);
    }

    let mut characters = BTreeSet::new();
    let mut symbols = CatalogSymbols::default();
    for path in roots.iter().flat_map(|root| {
        ["executables", "libraries", "subsystems", "drivers"].map(|owner| root.join(owner))
    }) {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                visit(&path, &expanded, &mut characters, &mut symbols.directories)?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(path_error(&path, error)),
        }
    }

    for character in characters {
        match character as u32 {
            0x2e80..=0x9fff | 0xff00..=0xffef => symbols.cjk.push(character),
            _ => symbols.other.push(character),
        }
    }
    symbols.directories.sort();
    symbols.directories.dedup();
    Ok(symbols)
}

fn visit(
    path: &Path,
    locales: &BTreeSet<String>,
    characters: &mut BTreeSet<char>,
    directories: &mut Vec<PathBuf>,
) -> io::Result<()> {
    // A crate without catalogs must be watched so its first locales directory
    // invalidates the previous subset without requiring a manifest edit.
    if path
        .join("Cargo.toml")
        .try_exists()
        .map_err(|error| path_error(path, error))?
        && !path
            .join("locales")
            .try_exists()
            .map_err(|error| path_error(path, error))?
    {
        directories.push(path.to_owned());
    }
    for entry in fs::read_dir(path).map_err(|error| path_error(path, error))? {
        let entry = entry.map_err(|error| path_error(path, error))?;
        // DirEntry::file_type does not follow symlinks.
        if !entry
            .file_type()
            .map_err(|error| path_error(&entry.path(), error))?
            .is_dir()
            || entry.file_name() == "target"
            || entry.file_name() == "node_modules"
        {
            continue;
        }
        let child = entry.path();
        if entry.file_name() != "locales" {
            visit(&child, locales, characters, directories)?;
            continue;
        }
        directories.push(child.clone());
        for locale in locales {
            let catalog = child.join(format!("{locale}.json"));
            let source = match fs::read_to_string(&catalog) {
                Ok(source) => source,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(path_error(&catalog, error)),
            };
            let values: serde_json::Value = serde_json::from_str(&source).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid translation JSON in {}: {error}", catalog.display()),
                )
            })?;
            let values = values.as_object().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Expected a translation object in {}", catalog.display()),
                )
            })?;
            for (key, value) in values {
                collect_translation_characters(key, value, &catalog, characters)?;
            }
        }
    }
    Ok(())
}

/// Traverse flat or nested JSON catalogs without treating group objects as strings.
fn collect_translation_characters(
    key: &str,
    value: &serde_json::Value,
    catalog: &Path,
    characters: &mut BTreeSet<char>,
) -> io::Result<()> {
    match value {
        serde_json::Value::String(text) => {
            characters.extend(text.chars().filter(|character| *character >= '\u{a0}'));
            Ok(())
        }
        serde_json::Value::Object(children) => {
            for (segment, child) in children {
                let nested_key = format!("{key}.{segment}");
                collect_translation_characters(&nested_key, child, catalog, characters)?;
            }
            Ok(())
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Non-string translation for {key:?} in {}",
                catalog.display()
            ),
        )),
    }
}

fn path_error(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "xila-font-catalogs-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("Cannot create fixture {}: {error}", path.display()),
                }
            }
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn combines_core_and_external_application_catalogs() {
        let core = Fixture::new();
        let applications = Fixture::new();
        core.write("libraries/core/locales/zh-CN.json", r#"{"key":"核心"}"#);
        applications.write("executables/clock/locales/zh-CN.json", r#"{"key":"时钟"}"#);
        let symbols = collect_multiple(&[&core.0, &applications.0], &["zh-CN"]).unwrap();
        for character in ['核', '心', '时', '钟'] {
            assert!(symbols.cjk.contains(character));
        }
        assert_eq!(symbols.directories.len(), 2);
    }

    #[test]
    fn collects_only_translation_values_including_punctuation_and_plurals() {
        let root = Fixture::new();
        root.write(
            "executables/app/locales/zh-CN.json",
            r#"{"Unused key 字":"错误：{}。","Count.other":"共 {count} 项"}"#,
        );
        root.write("libraries/shared/locales/zh-CN.json", r#"{"Error":"错误"}"#);
        root.write(
            "subsystems/ui/locales/en.json",
            r#"{"Label":"English only"}"#,
        );

        let symbols = collect(&root.0, &["zh-CN", "zh", "en"]).unwrap();
        assert_eq!(symbols.cjk, "。共误错项：");
        assert_eq!(symbols.other, "");
        assert_eq!(symbols.directories.len(), 3);
    }

    #[test]
    fn keeps_non_cjk_punctuation_in_primary_font_and_collects_fallbacks() {
        let root = Fixture::new();
        root.write("executables/app/locales/zh-CN.json", r#"{"Text":"“中”"}"#);
        root.write("executables/app/locales/fr.json", r#"{"Text":"é"}"#);

        let symbols = collect(&root.0, &["zh-CN", "fr"]).unwrap();
        assert_eq!(symbols.cjk, "中");
        assert_eq!(symbols.other, "é“”");
    }

    #[test]
    fn collects_translated_glyphs_from_nested_catalogs_and_plural_groups() {
        let root = Fixture::new();
        root.write(
            "executables/app/locales/zh-CN.json",
            r#"{"menu":{"title":"设置："},"count":{"one":"项","other":"多项。"}}"#,
        );
        let symbols = collect(&root.0, &["zh-CN"]).unwrap();
        for character in ['设', '置', '：', '项', '多', '。'] {
            assert!(
                symbols.cjk.contains(character),
                "missing glyph: {character}"
            );
        }
    }

    #[test]
    fn watches_existing_crate_before_its_first_catalog() {
        let root = Fixture::new();
        root.write(
            "executables/new_app/Cargo.toml",
            "[package]\nname = \"new_app\"\n",
        );
        fs::create_dir(root.0.join("executables/new_app/src")).unwrap();
        let symbols = collect(&root.0, &["zh-CN"]).unwrap();
        assert!(
            symbols
                .directories
                .contains(&root.0.join("executables/new_app"))
        );

        root.write("executables/new_app/locales/zh-CN.json", r#"{"New":"新"}"#);
        let symbols = collect(&root.0, &["zh-CN"]).unwrap();
        assert_eq!(symbols.cjk, "新");
        assert_eq!(
            symbols.directories,
            vec![root.0.join("executables/new_app/locales")]
        );
    }

    #[test]
    fn expands_normalized_and_base_locales_and_comma_separated_fallbacks() {
        let root = Fixture::new();
        for (locale, value) in [
            ("zh-CN", "中"),
            ("zh", "文"),
            ("fr-CA", "ê"),
            ("fr", "é"),
            ("de", "ü"),
            ("en", "\u{a0}"),
            ("ja", "外"),
        ] {
            root.write(
                &format!("drivers/display/locales/{locale}.json"),
                &format!(r#"{{"Text":"{value}"}}"#),
            );
        }
        let symbols = collect(&root.0, &[" ZH_cn ", " FR-ca, DE , en, ", "zh-CN"]).unwrap();
        assert_eq!(symbols.cjk, "中文");
        assert_eq!(symbols.other, "\u{a0}éêü");
    }

    #[test]
    fn rejects_invalid_json_and_non_string_translations_with_catalog_path() {
        let root = Fixture::new();
        for source in ["{", r#"{"Count":3}"#, r#"{"Count":{"other":3}}"#, "[]"] {
            root.write("libraries/shared/locales/en.json", source);
            let error = collect(&root.0, &["en"]).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(
                error
                    .to_string()
                    .contains("libraries/shared/locales/en.json")
            );
        }
    }

    #[test]
    fn skips_build_and_dependency_directories() {
        let root = Fixture::new();
        for directory in ["target", "node_modules"] {
            root.write(
                &format!("libraries/{directory}/ignored/locales/en.json"),
                "invalid JSON must not be read",
            );
        }
        let symbols = collect(&root.0, &["en"]).unwrap();
        assert!(symbols.directories.is_empty());
        assert!(symbols.cjk.is_empty());
        assert!(symbols.other.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlink_directories() {
        let root = Fixture::new();
        root.write("outside/locales/en.json", "invalid JSON must not be read");
        fs::create_dir(root.0.join("libraries")).unwrap();
        std::os::unix::fs::symlink(root.0.join("outside"), root.0.join("libraries/linked"))
            .unwrap();
        std::os::unix::fs::symlink(root.0.join("outside"), root.0.join("drivers")).unwrap();
        let symbols = collect(&root.0, &["en"]).unwrap();
        assert!(symbols.directories.is_empty());
    }
}
