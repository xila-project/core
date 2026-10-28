# Localization conventions

Xila uses [embedded-i18n](https://github.com/AlixANNERAUD/embedded-i18n) 0.1.3
with compile-time JSON translations. Each crate owns its own Tolgee namespace
and eight JSON_C catalogs in its local 'locales/' directory. These files use\n**nested JSON objects** to avoid repeating dotted key prefixes; the semantic\nidentifiers used by Rust macros and Tolgee remain unchanged.

## Naming

Use lowercase snake_case segments separated by dots: 'feature.component.purpose'.
Use stable meaning-based names, **never English UI text as a key**. Do not repeat
the crate/namespace prefix inside each key. The same English label may require
different IDs when one occurrence represents a state and another represents an
action (for example, 'open' as a verb versus an adjective).

- Copy changes do not automatically change the ID.
- A meaning or placeholder-contract change requires a new ID and translation
  review. Keep placeholder order, meaning and units clear to translators.
- For CLDR plurals, keep forms as 'key.one', 'key.few', 'key.many',
  'key.other', etc. Locale-specific categories (notably Polish) must survive
  both extraction and synchronization.
- Do not localize protocol commands, machine-readable identifiers, file
  formats, environment-variable names or wire-level errors.
- C-string keys using 'translate!(c"...")' must continue returning a
  statically allocated CStr; avoid converting them to runtime strings.
- Context-dependent strings should not share translation keys by default.

Use per-crate 'en.json' as source-of-truth for English values, but retain
all other language values exactly. The other catalogs may contain additional
CLDR plural categories. Use Tolgee's description field for ambiguity,
placeholders, domain-specific terms, or the intended UI location; that
metadata does not belong in the embedded runtime catalogs.

## Catalog structure and Tolgee

Group translations by their dotted semantic identifiers:

```json
{
  "alarm": {
    "controls": {
      "delete": "Delete"
    }
  },
  "stopwatch": {
    "laps": {
      "recorded": {
        "one": "1 lap recorded",
        "other": "{count} laps recorded"
      }
    }
  }
}
```

A key such as `alarm.controls.delete` is **still** accessed with
`translate!("alarm.controls.delete")`. Both embedded-i18n and Tolgee flatten
nested groups to dotted IDs; nesting changes source representation, not runtime
lookups, translation keys or firmware tables. CLDR plural suffixes are leaves.

The Tolgee CLI retains `format: "JSON_C"` for the existing C-style message
format, but **must** export nested keys: `pull.delimiter: "."` in
`tolgee.config.ts`. The old empty delimiter would export flat JSON and
undo the layout during `tolgee pull`. The configured `push.files` can
import nested JSON as the same dotted keys; `push.removeOtherKeys` stays
`false`.

Tolgee documents a potential ambiguity when a key is both a string and a
parent (for example `menu` and `menu.title`), or when mixed flat/nested
JSON declares the same key twice. The shared `flattenCatalog` and static
checks reject those cases, rather than silently losing translations.
Do not use `pull --empty-dir` during migration; inspect the diff and run
`node tools/localization/check.mjs` after a pull and before a push.

Reference: https://docs.tolgee.io/platform/formats/structured_json
and https://docs.tolgee.io/tolgee-cli/project-configuration.

## Migration

The machine-readable old-to-new manifests are under 'mappings/' and are
namespaced. 'split_review' records keys duplicated to distinguish different
UI contexts. The original translation text is intentionally preserved.
'legacy.*' entries are existing untranslated/unused catalog entries retained
for manual review. The empty bootsplash namespace has no mappings.

This migration intentionally makes a **breaking change to Tolgee keys**;
there is no special remote key-rename or translation-history migration.
Do not run an automatic remove-unused sweep without separately reviewing
its impact. For routine updates follow the nested JSON_C push/pull configuration,
validate the resulting catalogs, and inspect `tolgee compare` first.

## Checks

    node --test tolgee.extractor.test.mjs tools/localization/catalog.test.mjs
    node tools/localization/check.mjs
    node tools/localization/check.mjs --baseline <PRE_MIGRATION_SHA>
    EMBEDDED_I18N_STRICT=1 mise run localization:check

The optional baseline mode compares **every translation value** in each
pre-migration catalog to its new mapped destination and checks duplicated
contexts. It needs the original Git commit locally available.

The static checker enforces key syntax, namespace references, catalog presence,
and collisions. It reports unused keys without failing: pre-existing unused
entries require contextual review rather than automatic deletion. The
embedded-i18n strict builds validate locale-specific plurals, format arguments
and fallback conditions. All firmware-size claims require real target builds;
these changes do not add runtime translation tables or additional locale data.

Japanese (`ja`) is an experimental compile-time locale. Japanese Core and
Applications catalogs must be built together, and reviewers should check the
translations and UI layout before release. For graphics, download the pinned
Source Han Sans JP build input with `bash tools/localization/fetch-japanese-font.sh`.
When Core is a Git dependency, pass its absolute path in
`XILA_JAPANESE_FONT_PATH`. Set `XILA_FONT_CATALOG_ROOTS` to the Applications
checkout so its translated glyphs are included. See
`subsystems/graphics/fonts/README.md` for coverage and target-size limitations.
