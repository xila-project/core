# Localization builds

The shipped locales are `en`, `fr`, `es`, `de`, `pt-BR`, `it`, and `pl`.
Install the workspace's pinned Node dependencies with `npm ci` before building
graphics. Use the development environment and Rust version pinned by CI.

```sh
export CARGO_TARGET_DIR="$PWD/target/localization"
export EMBEDDED_I18N_STRICT=1
for locale in en fr es de pt-BR it pl; do
  EMBEDDED_I18N_LOCALE="$locale" cargo make check-localization
done
```

Host, guest applications, fonts, and the LittleFS image must use the same locale.
Reusing the Cargo target directory does not make an existing disk/image adopt a
new locale: rebuild the appropriate image and restore/reseed it when needed.
Strict translation checks and CI's changed-locale selection remain unchanged.

## Font coverage and caching

`subsystems/graphics/build.rs` unions the primary locale's glyph ranges with
`EMBEDDED_I18N_FALLBACK` (English by default). Comma-separated fallback chains
are supported, for example `EMBEDDED_I18N_LOCALE=en EMBEDDED_I18N_FALLBACK=fr,de`.
Empty fallback entries are ignored. Locale spelling is normalized and regional
locales use known parent ranges if necessary. Unknown locales fail the build.

The seven shipped locales have four distinct, nested coverage sets. Graphics
generates all four sets once per generation-input configuration inside its Cargo
`OUT_DIR/generated_fonts`. Other locales supported by embedded-i18n's range API
generate their effective coverage on demand, without precomputing arbitrary
language combinations. This is build-time primary/fallback selection, not runtime
switching through `EMBEDDED_I18N_LOCALES`.

Each source has a stable range/input-specific path and a completion manifest.
SHA-256 identities include actual font bytes, converter implementation and
resolved installed dependencies, Node executable/version, lockfile, generation
logic, sizes, ranges and rendering options. Font Awesome changes affect only
the Montserrat sources that use it. Cached files must be nonempty and match their
recorded content hashes. Partial or missing output is regenerated; completion
is recorded only after successful generation. No private brand assets are
fetched, and generated assets are never checked into Git.

Changing only locale/fallback reuses valid C sources. The active archive is
recreated from exactly six selected source paths, not a recursive directory scan.
Unselected generated fonts remain disk files, with no compiled registry or
runtime references. LVGL's archive and bindings do not depend on locale; guest
and application binding generators remain bindgen-only.

`cargo clean` or another Cargo configuration may discard/change `OUT_DIR`.
That simply requires regeneration; correctness never depends on persistence.
The localization CI job maps its Rust cache to `target/localization` and enables
workspace-crate caching so graphics' generated outputs can survive cache pruning.
Cache restoration is an optimization, not a prerequisite for correct builds.

## Regressions and linking evidence

```sh
cargo test --manifest-path subsystems/graphics/tests/font_pipeline/Cargo.toml
node --test subsystems/graphics/tests/font_pipeline/*.test.mjs
python3 -m unittest discover -s subsystems/graphics/tests/font_pipeline -p 'test_*.py' -v
python3 subsystems/graphics/tests/font_pipeline/verify.py
```

The native verifier requires `ar`, `nm`, `size` and a linker supporting `-Map`.
It switches English → French → English without cleaning and then checks English
with French fallback. It records glyph tests, source hashes/mtimes, archive
members, linker maps, bitmap symbol sizes, executable section sizes, and LVGL
archive/bindings hashes/mtimes. Evidence defaults to `target/font-pipeline-evidence`
and CI uploads it. These are correctness checks, not a CI-speedup benchmark or
embedded-target validation.
