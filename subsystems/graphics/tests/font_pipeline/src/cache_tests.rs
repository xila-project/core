use crate::{cache, generate};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "xila-font-rust-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn node_lookup_preserves_the_path_entry_for_cargo_tracking() {
    let directory = Directory::new();
    let node = generate::node_path(&std::env::var_os("PATH").unwrap()).unwrap();
    let link = directory.0.join("node");
    std::os::unix::fs::symlink(node, &link).unwrap();
    assert_eq!(generate::node_path(directory.0.as_os_str()).unwrap(), link);
}

#[test]
fn cache_reuses_valid_outputs_and_recovers_invalid_ones() {
    let directory = Directory::new();
    let path = directory.0.join("font.c");
    let generate = |output: &std::path::Path| fs::write(output, "complete font");
    assert!(cache::ensure_output(&path, "a", generate).unwrap());
    let marker = path.with_extension("c.json");
    let before = (
        fs::metadata(&path).unwrap().modified().unwrap(),
        fs::metadata(&marker).unwrap().modified().unwrap(),
    );
    assert!(!cache::ensure_output(&path, "a", |_| panic!("must not regenerate")).unwrap());
    assert_eq!(
        before,
        (
            fs::metadata(&path).unwrap().modified().unwrap(),
            fs::metadata(&marker).unwrap().modified().unwrap()
        )
    );
    assert!(cache::ensure_output(&path, "b", generate).unwrap());
    for bytes in ["", "partial font"] {
        fs::write(&path, bytes).unwrap();
        assert!(cache::ensure_output(&path, "b", generate).unwrap());
    }
    for metadata in ["{broken", "null", "{}"] {
        fs::write(&marker, metadata).unwrap();
        assert!(cache::ensure_output(&path, "b", generate).unwrap());
    }
    fs::remove_file(&path).unwrap();
    assert!(cache::ensure_output(&path, "b", generate).unwrap());
    fs::remove_file(&marker).unwrap();
    assert!(cache::ensure_output(&path, "b", generate).unwrap());
}

#[test]
fn failed_and_empty_generation_never_publish_success() {
    let directory = Directory::new();
    let path = directory.0.join("font.c");
    assert!(
        cache::ensure_output(&path, "a", |output| {
            fs::write(output, "partial")?;
            Err(std::io::Error::other("converter failed"))
        })
        .is_err()
    );
    assert!(!path.with_extension("c.json").exists());
    assert!(cache::ensure_output(&path, "a", |output| fs::write(output, "")).is_err());
    assert!(!path.with_extension("c.json").exists());
}

#[test]
fn fingerprints_include_dependency_contents_and_field_boundaries() {
    let directory = Directory::new();
    let root = &directory.0;
    for (relative, name, dependencies) in [
        (
            "node_modules/converter",
            "converter",
            r#"{"nested":"1","shared":"1"}"#,
        ),
        ("node_modules/converter/node_modules/nested", "nested", "{}"),
        ("node_modules/shared", "shared", "{}"),
    ] {
        let package = root.join(relative);
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            format!(r#"{{"name":"{name}","version":"1","dependencies":{dependencies}}}"#),
        )
        .unwrap();
        fs::write(package.join("index.js"), "module.exports = 1;").unwrap();
    }
    let converter = root.join("node_modules/converter");
    let files = cache::converter_inputs(&converter).unwrap();
    let first = cache::input_digest(&files).unwrap();
    assert_eq!(first, cache::input_digest(&files).unwrap());
    fs::write(
        converter.join("node_modules/nested/index.js"),
        "module.exports = 2;",
    )
    .unwrap();
    let second = cache::input_digest(&files).unwrap();
    assert_ne!(first, second);
    fs::write(
        root.join("node_modules/shared/index.js"),
        "module.exports = 3;",
    )
    .unwrap();
    assert_ne!(second, cache::input_digest(&files).unwrap());
    assert_ne!(cache::digest(&[b"ab", b"c"]), cache::digest(&[b"a", b"bc"]));
}

#[test]
fn plans_have_unique_variant_symbols_and_font_scoped_inputs() {
    let directory = Directory::new();
    let root = &directory.0;
    fs::create_dir(root.join("fonts")).unwrap();
    for name in [
        "Montserrat-Regular.ttf",
        "unscii-16-full.woff",
        "FontAwesome5-Solid+Brands+Regular.woff",
    ] {
        fs::write(root.join("fonts").join(name), name).unwrap();
    }
    let english = crate::fonts::effective_ranges("en", "en").unwrap();
    let french = crate::fonts::effective_ranges("fr", "en").unwrap();
    let before = generate::sources(root, &root.join("out"), &english, "converter-v1").unwrap();
    let same = generate::sources(root, &root.join("out"), &english, "converter-v1").unwrap();
    assert_eq!(before, same);
    assert_eq!(before.len(), 6);
    for (source, size) in before.iter().zip([10, 14, 18, 28, 8, 16]) {
        let args = source
            .arguments
            .iter()
            .map(|s| s.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            &args[..9],
            &[
                "--bpp",
                "4",
                "--size",
                &size.to_string(),
                "--no-compress",
                "--stride",
                "0",
                "--align",
                "1"
            ]
        );
        assert_eq!(
            args.iter().any(|s| s.starts_with("61441,")),
            size != 8 && size != 16
        );
    }
    let other = generate::sources(root, &root.join("out"), &french, "converter-v1").unwrap();
    assert!(
        before
            .iter()
            .zip(&other)
            .all(|(a, b)| a.name != b.name && a.path != b.path)
    );
    let converter = generate::sources(root, &root.join("out"), &english, "converter-v2").unwrap();
    assert!(before.iter().zip(&converter).all(|(a, b)| a.path != b.path));
    fs::write(
        root.join("fonts/FontAwesome5-Solid+Brands+Regular.woff"),
        "new icons",
    )
    .unwrap();
    let icons = generate::sources(root, &root.join("out"), &english, "converter-v1").unwrap();
    assert!(
        before[..4]
            .iter()
            .zip(&icons)
            .all(|(a, b)| a.path != b.path)
    );
    assert_eq!(before[4..], icons[4..]);
    fs::write(root.join("fonts/unscii-16-full.woff"), "new unscii").unwrap();
    let unscii = generate::sources(root, &root.join("out"), &english, "converter-v1").unwrap();
    assert_eq!(icons[..4], unscii[..4]);
    assert!(
        icons[4..]
            .iter()
            .zip(&unscii[4..])
            .all(|(a, b)| a.path != b.path)
    );
}

#[test]
fn chinese_plans_subset_catalogs_and_cache_font_and_symbol_changes() {
    let directory = Directory::new();
    let root = &directory.0;
    fs::create_dir(root.join("fonts")).unwrap();
    for name in [
        "Montserrat-Regular.ttf",
        "unscii-16-full.woff",
        "FontAwesome5-Solid+Brands+Regular.woff",
        "SourceHanSansSC-Regular.otf",
    ] {
        fs::write(root.join("fonts").join(name), name).unwrap();
    }
    let ranges = crate::fonts::effective_ranges("zh-CN", "en").unwrap();
    let mut symbols = crate::catalogs::CatalogSymbols {
        cjk: "中。".into(),
        other: "“”".into(),
        directories: Vec::new(),
    };
    let before = generate::sources_with_catalogs(
        root,
        &root.join("out"),
        &ranges,
        "converter",
        Some(&symbols),
    )
    .unwrap();
    for (index, source) in before.iter().enumerate() {
        let args: Vec<_> = source
            .arguments
            .iter()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert!(args.windows(2).any(|args| args == ["--symbols", "中。"]));
        assert!(args.windows(2).any(|args| args == ["--symbols", "“”"]));
        assert!(
            !args.contains(&"19968-40959"),
            "must not embed the whole Han range"
        );
        assert_eq!(
            source
                .inputs
                .iter()
                .any(|path| path.ends_with("SourceHanSansSC-Regular.otf")),
            index < 4
        );
    }
    assert_eq!(
        before,
        generate::sources_with_catalogs(
            root,
            &root.join("out"),
            &ranges,
            "converter",
            Some(&symbols)
        )
        .unwrap()
    );
    symbols.cjk.push('文');
    let changed = generate::sources_with_catalogs(
        root,
        &root.join("out"),
        &ranges,
        "converter",
        Some(&symbols),
    )
    .unwrap();
    assert!(
        before
            .iter()
            .zip(&changed)
            .all(|(a, b)| a.name == b.name && a.path != b.path)
    );
    fs::write(
        root.join("fonts/SourceHanSansSC-Regular.otf"),
        "changed font",
    )
    .unwrap();
    let changed_font = generate::sources_with_catalogs(
        root,
        &root.join("out"),
        &ranges,
        "converter",
        Some(&symbols),
    )
    .unwrap();
    assert!(
        changed[..4]
            .iter()
            .zip(&changed_font)
            .all(|(a, b)| a.path != b.path)
    );
    assert_eq!(changed[4..], changed_font[4..]);
}

#[test]
fn converter_directory_can_be_supplied_by_a_consuming_workspace() {
    use std::path::{Path, PathBuf};
    let root = Path::new("/checkout/core/subsystems/graphics");
    assert_eq!(
        generate::converter_directory(root, Some(Path::new("/consumer/node_modules/lv_font_conv"))),
        PathBuf::from("/consumer/node_modules/lv_font_conv")
    );
    assert_eq!(
        generate::converter_directory(root, None),
        root.join("../../node_modules/lv_font_conv")
    );
}

#[test]
fn japanese_plans_subset_kana_and_kanji_with_jp_glyphs_in_every_font() {
    let directory = Directory::new();
    let root = &directory.0;
    fs::create_dir(root.join("fonts")).unwrap();
    for name in [
        "Montserrat-Regular.ttf",
        "unscii-16-full.woff",
        "FontAwesome5-Solid+Brands+Regular.woff",
        "SourceHanSansJP-Regular.otf",
    ] {
        fs::write(root.join("fonts").join(name), name).unwrap();
    }

    let ranges = crate::fonts::effective_ranges("ja", "en").unwrap();
    assert!(crate::fonts::uses_catalog_subset("ja", "en"));
    assert_eq!(
        crate::fonts::catalog_font("ja", "en"),
        Some("SourceHanSansJP-Regular.otf")
    );
    let symbols = crate::catalogs::CatalogSymbols {
        cjk: "日本語あアイ。".into(),
        other: "“”".into(),
        directories: Vec::new(),
    };
    let sources = generate::sources_with_locale_catalogs(
        root,
        &root.join("out"),
        &ranges,
        "converter",
        Some(&symbols),
        std::path::Path::new("SourceHanSansJP-Regular.otf"),
        true,
    )
    .unwrap();
    assert_eq!(sources.len(), 6);
    for source in sources {
        assert!(
            source
                .inputs
                .iter()
                .any(|p| p.ends_with("SourceHanSansJP-Regular.otf"))
        );
        let args: Vec<_> = source
            .arguments
            .iter()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--symbols", "日本語あアイ。"])
        );
        assert!(
            !args.contains(&"19968-40959"),
            "must not rasterize every kanji"
        );
        assert!(
            !args.contains(&"12352-12543"),
            "must not rasterize every kana"
        );
    }
}

#[test]
fn japanese_catalog_font_is_preferred_to_english_fallback() {
    assert_eq!(
        crate::fonts::catalog_font("ja", "en"),
        Some("SourceHanSansJP-Regular.otf")
    );
    assert_eq!(
        crate::fonts::catalog_font("en", "ja"),
        Some("SourceHanSansJP-Regular.otf")
    );
    assert_eq!(crate::fonts::catalog_font("en", "fr"), None);
}

#[test]
fn japanese_font_can_be_supplied_from_a_consumer_checkout() {
    let directory = Directory::new();
    let root = &directory.0;
    fs::create_dir(root.join("fonts")).unwrap();
    for name in [
        "Montserrat-Regular.ttf",
        "unscii-16-full.woff",
        "FontAwesome5-Solid+Brands+Regular.woff",
    ] {
        fs::write(root.join("fonts").join(name), name).unwrap();
    }
    let supplied = root.join("provided-font.otf");
    fs::write(&supplied, b"test font").unwrap();
    let symbols = crate::catalogs::CatalogSymbols {
        cjk: "日本".into(),
        other: String::new(),
        directories: Vec::new(),
    };
    let sources = generate::sources_with_locale_catalogs(
        root,
        &root.join("out"),
        &crate::fonts::effective_ranges("ja", "en").unwrap(),
        "converter",
        Some(&symbols),
        &supplied,
        true,
    )
    .unwrap();
    assert!(
        sources
            .iter()
            .all(|source| source.inputs.contains(&supplied))
    );
}
