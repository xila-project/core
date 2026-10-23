#[path = "../../../build/fonts.rs"]
pub mod fonts;

#[cfg(test)]
mod tests {
    use super::fonts::{effective_ranges, supported_variants};

    #[test]
    fn english_includes_required_extras() {
        assert_eq!(
            effective_ranges("en", "en").unwrap(),
            vec![0x20..0x7f, 0xb0..0xb1, 0x2022..0x2023]
        );
    }

    #[test]
    fn fallback_chains_and_regional_locales_union_coverage() {
        let french = vec![0x20..0x7f, 0xa0..0x250, 0x2022..0x2023];
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
            vec![0x20..0x7f, 0xa0..0x100, 0x2022..0x2023]
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
                vec![0x20..0x7f, 0xa0..0x180, 0x2022..0x2023]
            );
        }
    }

    #[test]
    fn upstream_non_shipped_coverage_remains_supported() {
        assert_eq!(
            effective_ranges("en", "ru,ru,en").unwrap(),
            vec![0x20..0x7f, 0xb0..0xb1, 0x400..0x530, 0x2022..0x2023]
        );
    }
}
