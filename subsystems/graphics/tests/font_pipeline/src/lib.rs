#[path = "../../../build/cache.rs"]
pub mod cache;
#[path = "../../../build/catalogs.rs"]
pub mod catalogs;
#[path = "../../../build/config.rs"]
pub mod config;
#[path = "../../../build/fonts.rs"]
pub mod fonts;
#[path = "../../../build/generate.rs"]
pub mod generate;

#[cfg(test)]
#[allow(
    clippy::disallowed_types,
    reason = "host-only test tool uses std atomics"
)]
mod cache_tests;

#[cfg(test)]
mod tests {
    use super::fonts::{effective_ranges, supported_variants};

    #[test]
    fn cache_digest_retains_sha256_hex_encoding() {
        assert_eq!(
            super::cache::digest(&[b"abc"]),
            "c3494ca1a2cf8eeb8a11ded316fb55b83c3bbbedb6313cd50415251e5d09e12f"
        );
    }

    #[test]
    fn english_includes_required_extras() {
        assert_eq!(
            effective_ranges("en", "en").unwrap(),
            vec![0x20..0x7f, 0xb0..0xb1, 0x2019..0x201a, 0x2022..0x2023]
        );
    }

    #[test]
    fn fallback_chains_and_regional_locales_union_coverage() {
        let french = vec![0x20..0x7f, 0xa0..0x250, 0x2019..0x201a, 0x2022..0x2023];
        for (locale, fallback) in [
            ("en", "fr"),
            ("fr", "en"),
            (" pt_br ", "en, fr, ,de"),
            ("fr-CA", "en"),
        ] {
            assert_eq!(effective_ranges(locale, fallback).unwrap(), french);
        }
        assert_eq!(
            effective_ranges("es", "").unwrap(),
            vec![0x20..0x7f, 0xa0..0x100, 0x2019..0x201a, 0x2022..0x2023]
        );
        assert!(effective_ranges("xx", "en").is_err());
        assert!(effective_ranges("en", "xx").is_err());
    }

    #[test]
    fn shipped_pairs_share_four_effective_variants() {
        let variants = supported_variants();
        assert_eq!(variants.len(), 4);
        for primary in ["en", "fr", "es", "de", "pt-BR", "it", "pl"] {
            for fallback in ["en", "fr", "es", "de", "pt-BR", "it", "pl"] {
                assert!(variants.contains(&effective_ranges(primary, fallback).unwrap()));
            }
        }
        assert_eq!(
            effective_ranges("es", "en").unwrap(),
            effective_ranges("it", "en").unwrap()
        );
        for locale in ["de", "pt-BR", "pl"] {
            assert_eq!(
                effective_ranges(locale, "en").unwrap(),
                vec![0x20..0x7f, 0xa0..0x180, 0x2019..0x201a, 0x2022..0x2023]
            );
        }
    }

    #[test]
    fn upstream_non_shipped_coverage_remains_supported() {
        assert_eq!(
            effective_ranges("en", "ru,ru,en").unwrap(),
            vec![0x20..0x7f, 0xb0..0xb1, 0x400..0x530, 0x2019..0x201a, 0x2022..0x2023]
        );
    }

    #[test]
    fn terminal_fonts_use_only_approved_sources() {
        let graphics_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fonts_dir = graphics_root.join("fonts");
        let out = graphics_root.join("unused");
        let unscii = fonts_dir.join("unscii-16.woff");
        let terminal_inputs = |plan: Vec<super::generate::Source>,
                               expected: &[std::path::PathBuf]| {
            let terminal_fonts = plan
                .iter()
                .filter(|source| source.name.starts_with("font_unscii_"))
                .collect::<Vec<_>>();
            assert_eq!(terminal_fonts.len(), 2);
            for source in terminal_fonts {
                assert_eq!(
                    source.inputs, expected,
                    "unexpected inputs for {}",
                    source.name
                );
            }
        };

        for ranges in supported_variants() {
            let plan =
                super::generate::sources(&graphics_root, &out, &ranges, "converter").unwrap();
            terminal_inputs(plan, std::slice::from_ref(&unscii));
        }

        for (locale, file) in [
            ("zh-CN", "SourceHanSansSC-Regular.otf"),
            ("zh", "SourceHanSansSC-Regular.otf"),
            ("ja", "SourceHanSansJP-Regular.otf"),
        ] {
            let ranges = effective_ranges(locale, "en").unwrap();
            assert_eq!(super::fonts::catalog_font(locale, "en"), Some(file));
            let catalog_font = std::path::Path::new(file);
            let symbols = super::catalogs::CatalogSymbols {
                cjk: "月午".into(),
                ..Default::default()
            };
            let plan = super::generate::sources_with_locale_catalogs(
                &graphics_root,
                &out,
                &ranges,
                "converter",
                Some(&symbols),
                catalog_font,
            )
            .unwrap();
            terminal_inputs(plan, &[unscii.clone(), fonts_dir.join(catalog_font)]);
        }
    }
}
