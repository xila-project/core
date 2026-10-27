# Project Overview

Xila is an embedded operating system written in Rust for low-power microcontrollers, including ESP32, nRF, and STM32 targets. The root Cargo manifest is a virtual workspace; `core/` contains the public `xila` façade and is the default member. This workspace separates runtime-owned services into `subsystems/`, reusable contracts and implementations into `libraries/`, generic devices into `drivers/core/`, and the Wasm host into `executables/wasm/`; code must account for constrained RAM, flash, and target capabilities.

# Build & Test Commands

- Format Rust: `mise run format:rust -- --check`
- Format TOML: `mise run format:toml -- --check`
- Format JSON: `npm exec prettier -- --check "**/*.json"`
- Run host and guest checks: `mise run check:all`
- Run host and guest Clippy: `mise run clippy:all`
- Test the workspace: `mise run test:workspace`
- For focused validation, use `mise run check:host`, `mise run check:guest`, `mise run clippy:host`, or `mise run clippy:guest` as appropriate.

Shared tasks are defined in `mise.toml`, developer-only tasks and tools in `mise.development.toml`, and CI tasks with task-scoped tools and package setup in `mise.ci.toml`. All task names use `<namespace>:<command>`. CI is the source of truth for target-specific validation.

- The default `development` environment provides task-scoped local tools; use `MISE_ENV=ci` for CI tasks. Version pins are centralized in `[vars]` in `mise.toml`; tasks select tool clusters with `extends`. Run tasks to install only their tools, or optionally preinstall all development task tools with `mise install --include-task-tools`. Run `mise bootstrap packages apply --manager apt` for local native packages; CI build tasks apply only their required package sets through dependencies, while formatting tasks install no native packages or build toolset. No development container is required.
- For localization, set `EMBEDDED_I18N_LOCALE` and `EMBEDDED_I18N_STRICT=1`, run `mise run localization:check`, and test extractor changes with `node --test tolgee.extractor.test.mjs`. Build host, guests, fonts and LittleFS images for the same locale; locale builds can share `CARGO_TARGET_DIR=target/localization`. Graphics' Rust build helpers cache four shipped font-range variants inside `OUT_DIR` and archive each font as a separate object with unique symbols. `src/fonts.rs` selects only the locale/fallback symbols at compile time; unselected font objects stay out of the executable. `EMBEDDED_I18N_FALLBACK` defaults to `en` and accepts comma-separated locales. For font generation, only `lv_font_conv` runs JavaScript. Run font regressions with `mise run test:fonts`. Cleaning build output safely regenerates the font variants. Keep `docs/` local-only.

# Architecture & Implementation Guidelines

- Visual identity is defined in `xila-project/brand`: Montserrat Regular 400 for proportional UI text, SemiBold 600 for web headings, and Bold 700 for the wordmark and emphasis. Preserve the embedded Regular glyph subsets and Unscii terminal font unless a task explicitly needs more weights. Browser fonts and README logos are local deployment copies; record their exact brand source revision when updating them. Never make builds fetch assets from the private brand repository.

- Put changes in the crate that owns the behavior and follow its existing APIs, error handling, feature flags, and tests.
- Keep reusable libraries portable: preserve `no_std` operation and isolate host-only code and dependencies behind target-specific Cargo sections or conditional compilation.
- Check a crate's `Cargo.toml`, features, and consumers before changing dependencies or enabling features. Preserve intentional `default-features = false`, optional dependencies, and target-specific dependency boundaries.
- Treat every byte as important: consider RAM, flash, stack, and peak allocation costs. Prefer bounded or caller-provided storage where appropriate; account for capacity, lifetime, and allocation behavior when using `Vec`, `String`, or `Box`. Avoid unnecessary allocations, copies, buffers, large static data, and dependencies.
- Classify crates by runtime ownership and lifecycle, not mere statefulness; see `subsystems/README.md` and `libraries/README.md`.
- Preserve the VFS polling I/O contract (`Poll<Result<usize>>` with task context): register wakeups, handle cancellation and avoid spinning on `Pending`; synchronous adapters probe once and return `RessourceBusy` if pending.
- Preserve per-crate Tolgee namespaces, Rust placeholders and CLDR plural categories. Do not translate machine-readable interfaces.
- Use stable dotted semantic translation IDs with lowercase snake_case segments (`feature.component.purpose`), not English copy as keys. Wording-only changes keep the ID; changes in meaning or placeholders require a new ID or explicit review. Avoid sharing one key across contexts with different translation requirements. Maintain mapping files under `tools/localization/mappings/` and run `node tools/localization/check.mjs` plus `node --test tolgee.extractor.test.mjs`. For historical migration verification, run `node tools/localization/check.mjs --baseline <pre-migration-commit>`. The `modules/` migration changed file-derived namespaces: leave `removeOtherKeys` disabled and do not perform destructive Tolgee sync without approval.
- Platform drivers, runtime assemblies, native UI debugging, seed images, profiling and Docker live in `xila-project/platforms`. Official applications and their catalogs live in `xila-project/applications`.
- Core integration tests can use Platforms as a development dependency. Preserve the root Cargo patches so local Core packages have one identity, and set `XILA_APPLICATIONS_MANIFEST` for external guest builds. Set `XILA_APPLICATIONS_ROOT` for localization lint and `XILA_FONT_CATALOG_ROOTS` for CJK font subsets.
- Identify the generator or build script for generated sources and assets before changing them. Keep generated files and build output out of source changes unless the task explicitly requires updating generated assets.

# Non-Negotiables

- Keep `docs/superpowers/` planning and design documents local-only. Never stage or commit them; preserve local copies when removing them from Git tracking.
- Make the smallest focused change that follows the existing crate structure and conventions.
- Validate changes with the narrowest relevant checks, using CI's host/guest checks as the source of truth for those targets. Host and guest checks do not establish hardware-target correctness; report what was actually checked and any target/toolchain limitations.
- Do not assume documentation in adjacent repositories is current; prefer this repository's source, manifests, and CI configuration as ground truth.
