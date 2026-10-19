# Localized builds

Xila embeds the selected locale **at compile time** through
[`embedded-i18n`](https://github.com/AlixANNERAUD/embedded-i18n). This keeps
runtime memory usage and the language-selection logic out of MCU builds.
English (`en`) is the fallback language.

The Core catalogs live beside each crate at `modules/**/locales/` or
`executables/**/locales/`; application-local strings belong in the
application namespace, not a global catalog.

| Locale | Status |
| --- | --- |
| `en` — English | Existing supported locale (fallback) |
| `fr` — French | Existing supported locale |
| `es` — Spanish | New translated catalogs; needs CI and native-language review before merging |
| `de` — German | Planned in #441 |
| `pt-BR` — Brazilian Portuguese | Planned in #441; locale ranges need upstream support |

## Build / verify a locale

Run the following **inside Xila's development container** to use the pinned
Rust, Wasm and LVGL toolchains:

```sh
export EMBEDDED_I18N_LOCALE=es
export EMBEDDED_I18N_STRICT=1

# Compile all host and bundled guest translation call sites.
cargo make check-localization

# Build LVGL/terminal font subsets for the same locale (incl. fallback).
cargo make generate-fonts

# Build the browser demo's bundled guest applications and LittleFS image.
cargo build -p calculator -p weather -p clock -p pixel_painter -p synth \
  --target wasm32-wasip1 --release
cargo make prepare-wasm-image

# Build the browser demo for this specific locale and corresponding image.
(cd examples/wasm && RUSTUP_TOOLCHAIN="$XILA_NIGHTLY_TOOLCHAIN" trunk build --release --public-url ./)
```

Both the guest programs and host must be compiled with the same
`EMBEDDED_I18N_LOCALE`. The locale-specific `seed.img` must accompany its
runtime; swapping a translated runtime over an existing filesystem does not
translate already-installed Wasm programs.

On `main`, CI builds and publishes matching reference images per supported
locale in `seed-images/{locale}/`; the localized browser demonstration is at
`pages/demonstrations/wasm/{locale}/`.

## Contributing translations

The source of truth for translation management and review is
[Tolgee](https://translate.anneraud.fr/engage/xila-core/), configured through
`tolgee.config.ts` and `tolgee.extractor.mjs`. Do **not** add a competing
extraction or translation-management tool.

- Sync English source strings first and translate within the existing crate
  namespaces.
- Keep Rust format placeholders exactly as used by the source:
  `{}`, `{:.2}`, `{:?}`, `{count}`, etc. Translation builds in strict
  mode should reject missing or invalid required entries.
- Do not translate commands, flags, protocol identifiers, paths,
  environment variables or other machine-readable interfaces.
- Review text expansion on small LVGL displays and glyph availability, not
  only Tolgee completion percentages.
- A translated `es.json` does **not** imply human review. Native speakers
  should check terminology, UI context and punctuation before release.

Adding more locales should happen **one language per pull request**. Before
shipping a locale, update CI localization checks, per-locale Wasm guest/image
builds and the matching font subset. See [#441](https://github.com/Xila-Project/Core/issues/441)
for the planned rollout.
