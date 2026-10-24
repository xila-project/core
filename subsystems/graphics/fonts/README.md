# Embedded fonts

Montserrat Regular supplies proportional Latin text at 10/14/18/28px, and
Unscii supplies terminal text at 8/16px. Font Awesome supplies UI icons.

## Simplified Chinese

`SourceHanSansSC-Regular.otf` is the unmodified Source Han Sans SC Regular 2.005
font from Adobe's [Source Han Sans](https://github.com/adobe-fonts/source-han-sans)
release branch, commit `a4f7cf94edfb9d7ffbdfc4841de276358bd7e0f2`, path
`OTF/SimplifiedChinese/SourceHanSansSC-Regular.otf`.

- SHA-256: `f1d8611151880c6c336aabeac4640ef434fa13cbfbf1ffe82d0a71b2a5637256`
- License: SIL OFL 1.1, retained in `SourceHanSans-LICENSE.txt`.
- The build never downloads fonts. The full OTF is a build input, not a runtime
  asset or a file copied into the filesystem image.

For `zh-CN` (or `zh`), `../build/catalogs.rs` gathers the distinct characters in
translation **values**, including plural entries, across the application,
library and subsystem catalogs. The selected locale, its base language and the
configured comma-separated fallbacks contribute characters. Cargo watches the locale directories
and crates without catalogs so edits and new namespaces regenerate the fonts.
Only the used Chinese characters and punctuation are merged from Source Han Sans
into the proportional font tables; Montserrat retains Latin glyphs and metrics.
Terminal tables keep Unscii, also subsetted to
catalog characters instead of the entire Han range.

For the initial 450-entry Chinese catalogs (411 distinct non-ASCII characters),
the six native font objects total 404,970 bytes of text/data, compared with
70,338 bytes for English (`size -t libfont_assets.a`, native debug build).
These are object-section sizes, not measured MCU flash or peak runtime RAM;
target-specific linking and validation are still necessary.

`embedded-i18n` 0.1.3 already recognizes `zh-CN`, but its generic ranges include
all of `U+4E00–U+9FFF` and omit Chinese punctuation. Catalog subsetting avoids
that full-range cost and includes punctuation such as `。` and `：`. Arbitrary
Chinese text outside the catalogs is not guaranteed to have a glyph. This is
compile-time localization, not runtime language switching; other locales retain
their existing generation path.

Check extraction and actual LVGL glyph lookup:

```sh
cargo test --manifest-path subsystems/graphics/tests/font_pipeline/Cargo.toml
EMBEDDED_I18N_LOCALE=zh-CN EMBEDDED_I18N_STRICT=1 \
  CARGO_TARGET_DIR="$PWD/target/localization/zh-CN" \
  cargo test --manifest-path subsystems/graphics/tests/font_link/Cargo.toml
EMBEDDED_I18N_LOCALE=zh-CN EMBEDDED_I18N_STRICT=1 \
  CARGO_TARGET_DIR=target/localization/zh-CN mise run localization:check
```

Chinese catalogs cover all current English namespaces, using the Chinese CLDR
`other` plural category. Native-speaker terminology review and embedded-target
flash/readability validation are still required before treating this as a
reviewed language release. Do not add Chinese to release/deployment bundles
solely because compilation passes. Build guest applications, host and seed image
with the same locale and use an independent disk fixture.

The four shipped Latin range variants retain the upstream Rust cache and unique
per-font archive symbols. Chinese adds only the active catalog-subset variant;
its selected characters and font contents participate in the cache key. Rust
selects the active symbols at compile time, so unselected font objects are not
linked. Locale builds may share `CARGO_TARGET_DIR=target/localization`; only
`lv_font_conv` runs JavaScript, never catalog discovery.
