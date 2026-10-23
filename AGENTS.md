# Project Overview

Xila is an embedded operating system written in Rust for low-power microcontrollers, including ESP32, nRF, and STM32 targets. This Cargo workspace separates runtime-owned services into `subsystems/`, reusable contracts and implementations into `libraries/`, platform support into `drivers/`, and applications/runtimes into `executables/`; code must account for constrained RAM, flash, and target capabilities.

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
- For localization, set `EMBEDDED_I18N_LOCALE` and `EMBEDDED_I18N_STRICT=1`, run `mise run localization:check`, and test extractor changes with `node --test tolgee.extractor.test.mjs`. Build host, guests, fonts and LittleFS images for the same locale; locale builds can share `CARGO_TARGET_DIR=target/localization`. Keep `docs/` local-only.

# Architecture & Implementation Guidelines

- Visual identity is defined in `xila-project/brand`: Montserrat Regular 400 for proportional UI text, SemiBold 600 for web headings, and Bold 700 for the wordmark and emphasis. Preserve the embedded Regular glyph subsets and Unscii terminal font unless a task explicitly needs more weights. Browser fonts and README logos are local deployment copies; record their exact brand source revision when updating them. Never make builds fetch assets from the private brand repository.

- Put changes in the crate that owns the behavior and follow its existing APIs, error handling, feature flags, and tests.
- Keep reusable libraries portable: preserve `no_std` operation and isolate host-only code and dependencies behind target-specific Cargo sections or conditional compilation.
- Check a crate's `Cargo.toml`, features, and consumers before changing dependencies or enabling features. Preserve intentional `default-features = false`, optional dependencies, and target-specific dependency boundaries.
- Treat every byte as important: consider RAM, flash, stack, and peak allocation costs. Prefer bounded or caller-provided storage where appropriate; account for capacity, lifetime, and allocation behavior when using `Vec`, `String`, or `Box`. Avoid unnecessary allocations, copies, buffers, large static data, and dependencies.
- Classify crates by runtime ownership and lifecycle, not mere statefulness; see `subsystems/README.md` and `libraries/README.md`.
- Preserve the VFS polling I/O contract (`Poll<Result<usize>>` with task context): register wakeups, handle cancellation and avoid spinning on `Pending`; synchronous adapters probe once and return `RessourceBusy` if pending.
- Preserve per-crate Tolgee namespaces, Rust placeholders and CLDR plural categories. Do not translate machine-readable interfaces. The `modules/` migration changed file-derived namespaces: leave `removeOtherKeys` disabled and do not perform destructive Tolgee sync without approval.
- For the containerized Wasm runtime, preserve capability-scoped HostFS mounts, state persistence, non-root execution and disabled-by-default networking; see `docs/runtime-image.md`.
- Keep target support explicit. Put hardware-specific adaptation in the relevant driver and keep runtime orchestration in subsystems and reusable contracts in libraries.
- When inspecting or improving the native graphical UI, use the Debug agent instead of relying on source inspection alone. Start `native_example` with `--display-driver headless` to work without X11 or Wayland (`--display-driver native` selects the windowed driver and remains the default). Headless mode starts the Debug transport in the background; then, as the same OS user, prefix these commands with `cargo run -p drivers_native --bin xila-debug --`: `diagnostics`, `resolution`, `screenshot /tmp/xila-ui.png`, and `ui-tree`. Use `pointer move X Y` followed by `pointer button press|release`, or `key NAME press|release` and `text TEXT`, to interact. The Debug application gets screen capture, UI state, and injected input through Xila graphics APIs; diagnostics use Xila task and memory APIs. Drivers supply output and physical inputs but are not queried or controlled by the Debug application. The client writes screenshot PNGs to the requested host path. JSON headers are capped at 64 KiB, screenshot payloads at 8 MiB, requests at 16 KiB, and printable ASCII text at 4096 bytes. The native control transport is available on Unix hosts only. See `examples/native/README.md` for the complete headless workflow.
- Identify the generator or build script for generated sources and assets before changing them. Keep generated files and build output out of source changes unless the task explicitly requires updating generated assets.

# Non-Negotiables

- Keep `docs/superpowers/` planning and design documents local-only. Never stage or commit them; preserve local copies when removing them from Git tracking.
- Make the smallest focused change that follows the existing crate structure and conventions.
- Validate changes with the narrowest relevant checks, using CI's host/guest checks as the source of truth for those targets. Host and guest checks do not establish hardware-target correctness; report what was actually checked and any target/toolchain limitations.
- Do not assume documentation in adjacent repositories is current; prefer this repository's source, manifests, and CI configuration as ground truth.

# Optimization and Profiling

- For optimization requests, define the workload first and record a native baseline before editing hot paths. Use the same inputs, locale, disk state, resolution, network mode, duration and machine for the comparison. Build outside the measured interval and run `mise run image:native` to build guest applications and the LittleFS seed image first.
- Build an optimized binary with symbols using `mise run build:profiling -- --features prebuilt-applications` for OS workloads (the feature is compatibility-only; omit it for independent benchmarks). Applications load from the disk image, not from the recorder's `--applications-dir` (used only for artifact hashing). Record uninstrumented measurements with `cargo run --manifest-path tools/profiling/Cargo.toml -- measure --scenario NAME --output target/profiles/BASELINE --runs 3 --duration 30 --applications-dir target/wasm32-wasip1/release-wasm -- target/profiling/native_example --terminal --no-network --seed-image target/seed/native/seed.img`. Existing disks are not re-seeded automatically: restore the same disk fixture between runs. Read `summary.json` and per-run logs; a duration limit is a bounded observation, not successful workload completion. Do not use failed or forcibly killed runs to claim improvements.
- For attribution, rebuild with `mise run build:profiling -- --features profiling,heaptrack`. The `profiling` feature records task/spawner/thread identity, cumulative poll CPU time, poll/wait wall time and allocation request volume in each run's `runtime.json`. The `heaptrack` feature annotates the native custom allocator. Features are disabled by default and Linux-only instrumentation compiles out on other targets.
- Use the recorder's `cpu` mode with separately installed Samply (or `--tool perf`) and `heap` mode with heaptrack. Read the README's profiling section for artifacts and limitations. Keep external profiler overhead and feature-enabled counters out of uninstrumented before/after consumption comparisons.
- CPU time, elapsed latency, RSS, allocator-used/reserved bytes and allocation request volume are different metrics. Task IDs can be reused: use the manager/generation identity in reports. Executor poll CPU includes task CPU; do not add them together. Snapshots are periodic, not final or peak heap reports. Check active poll/wait flags: long-running polls do not update completed CPU/counts until they return. Native improvements need follow-up validation on the intended Wasm/embedded target.
