#[cfg(test)]
mod tests {
    #[test]
    fn generated_font_symbols_link() {
        let fonts = [
            graphics::fonts::get_font_small() as *const _,
            graphics::fonts::get_font_medium() as *const _,
            graphics::fonts::get_font_large() as *const _,
            graphics::fonts::get_font_extra_large() as *const _,
            graphics::fonts::get_font_monospace_small() as *const _,
            graphics::fonts::get_font_monospace_medium() as *const _,
        ];
        std::hint::black_box(fonts);
    }

    #[test]
    fn selected_coverage_is_present_without_unrequested_accents() {
        let locale = option_env!("EMBEDDED_I18N_LOCALE").unwrap_or("en");
        let fallback = option_env!("EMBEDDED_I18N_FALLBACK").unwrap_or("en");
        let ranges =
            graphics_font_pipeline_test::fonts::effective_ranges(locale, fallback).unwrap();
        let accented = ranges.iter().any(|range| range.contains(&u32::from('é')));
        for font in [
            graphics::fonts::get_font_small(),
            graphics::fonts::get_font_medium(),
            graphics::fonts::get_font_large(),
            graphics::fonts::get_font_extra_large(),
            graphics::fonts::get_font_monospace_small(),
            graphics::fonts::get_font_monospace_medium(),
        ] {
            let has_glyph = |letter: char| {
                let mut descriptor = Default::default();
                unsafe {
                    font.get_glyph_dsc.expect("font descriptor callback")(
                        font,
                        &mut descriptor,
                        u32::from(letter),
                        0,
                    )
                }
            };
            for letter in [' ', '~', 'A', '°', '•'] {
                assert!(
                    has_glyph(letter),
                    "missing {letter:?} at {}px",
                    font.line_height
                );
            }
            assert_eq!(
                has_glyph('é'),
                accented,
                "accented coverage at {}px",
                font.line_height
            );
        }
        for font in [
            graphics::fonts::get_font_small(),
            graphics::fonts::get_font_medium(),
            graphics::fonts::get_font_large(),
            graphics::fonts::get_font_extra_large(),
        ] {
            let mut descriptor = Default::default();
            assert!(unsafe { font.get_glyph_dsc.unwrap()(font, &mut descriptor, 61441, 0) });
        }
    }

    #[test]
    fn localized_fonts_contain_chinese_and_punctuation() {
        let locale = option_env!("EMBEDDED_I18N_LOCALE").unwrap_or("en");
        let fallback = option_env!("EMBEDDED_I18N_FALLBACK").unwrap_or("en");
        if !graphics_font_pipeline_test::fonts::uses_chinese(locale, fallback) {
            return;
        }
        let symbols = graphics_font_pipeline_test::catalogs::collect(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../..")),
            &[locale, fallback],
        )
        .unwrap();
        assert!(symbols.cjk.contains('清') && symbols.cjk.contains('：'));
        for font in [
            graphics::fonts::get_font_small(),
            graphics::fonts::get_font_medium(),
            graphics::fonts::get_font_large(),
            graphics::fonts::get_font_extra_large(),
            graphics::fonts::get_font_monospace_small(),
            graphics::fonts::get_font_monospace_medium(),
        ] {
            let lookup = font.get_glyph_dsc.expect("Font glyph lookup");
            for character in symbols
                .cjk
                .chars()
                .chain(symbols.other.chars())
                .chain("A0°•".chars())
            {
                let mut descriptor = Default::default();
                assert!(
                    unsafe { lookup(font, &mut descriptor, character as u32, 0) },
                    "Missing glyph for {character} at {}px",
                    font.line_height
                );
            }
        }
    }
}
