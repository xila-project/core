# Localization conventions

Xila uses [embedded-i18n](https://github.com/AlixANNERAUD/embedded-i18n) 0.1.3
with compile-time JSON translations. Each crate owns its own Tolgee namespace
and nine JSON_C flat-key catalogs (including the experimental Japanese
locale) in its local 'locales/' directory.

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

## Migration

The machine-readable old-to-new manifests are under 'mappings/' and are
namespaced. 'split_review' records keys duplicated to distinguish different
UI contexts. The original translation text is intentionally preserved.
'legacy.*' entries are existing untranslated/unused catalog entries retained
for manual review. The empty bootsplash namespace has no mappings.

This migration intentionally makes a **breaking change to Tolgee keys**;
there is no special remote key-rename or translation-history migration.
Do not run an automatic remove-unused sweep without separately reviewing
its impact. For routine updates follow the existing Tolgee JSON_C push/pull
workflow, and inspect 'tolgee compare' first.

## Checks

    node --test tolgee.extractor.test.mjs
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

## Japanese (`ja`)

Japanese cardinal plurals use the CLDR `other` category. The initial
`ja.json` catalogs require native-speaker terminology and UI review before
release. Builds are compile-time localized: use the same locale for native
host, Wasm guests, graphics and image. For the JP-region CJK source font,
run `bash tools/localization/fetch-japanese-font.sh` before the Japanese
graphics build. See `subsystems/graphics/fonts/README.md`.
