# Project Overview

Xila is an embedded operating system written in Rust for low-power microcontrollers, including ESP32, nRF, and STM32 targets. This Cargo workspace separates runtime-owned services into `subsystems/`, reusable contracts and implementations into `libraries/`, platform support into `drivers/`, and applications/runtimes into `executables/`; code must account for constrained RAM, flash, and target capabilities.

# Build & Test Commands

- Format Rust: `cargo make format-rust -- --check`
- Format TOML: `cargo make format-toml --check`
- Format JSON: `npm exec prettier -- --check "**/*.json"`
- Run host and guest checks: `cargo make check`
- Run host and guest Clippy: `cargo make clippy`
- Test the workspace: `cargo make test`
- For focused validation, use `cargo make check-host`, `cargo make check-guest`, `cargo make clippy-host`, or `cargo make clippy-guest` as appropriate.

These tasks are defined in `Makefile.toml`; CI is the source of truth for target-specific validation.

# Architecture & Implementation Guidelines

- Visual identity is defined in `xila-project/brand`: Montserrat Regular 400 for proportional UI text, SemiBold 600 for web headings, and Bold 700 for the wordmark and emphasis. Preserve the embedded Regular glyph subsets and Unscii terminal font unless a task explicitly needs more weights. Browser fonts and README logos are local deployment copies; record their exact brand source revision when updating them. Never make builds fetch assets from the private brand repository.

- Put changes in the crate that owns the behavior and follow its existing APIs, error handling, feature flags, and tests.
- Keep reusable libraries portable: preserve `no_std` operation and isolate host-only code and dependencies behind target-specific Cargo sections or conditional compilation.
- Check a crate's `Cargo.toml`, features, and consumers before changing dependencies or enabling features. Preserve intentional `default-features = false`, optional dependencies, and target-specific dependency boundaries.
- Treat every byte as important: consider RAM, flash, stack, and peak allocation costs. Prefer bounded or caller-provided storage where appropriate; account for capacity, lifetime, and allocation behavior when using `Vec`, `String`, or `Box`. Avoid unnecessary allocations, copies, buffers, large static data, and dependencies.
- Keep target support explicit. Put hardware-specific adaptation in the relevant driver and keep runtime orchestration in subsystems and reusable contracts in libraries.
- Identify the generator or build script for generated sources and assets before changing them. Keep generated files and build output out of source changes unless the task explicitly requires updating generated assets.

# Non-Negotiables

- Keep `docs/superpowers/` planning and design documents local-only. Never stage or commit them; preserve local copies when removing them from Git tracking.
- Make the smallest focused change that follows the existing crate structure and conventions.
- Validate changes with the narrowest relevant checks, using CI's host/guest checks as the source of truth for those targets. Host and guest checks do not establish hardware-target correctness; report what was actually checked and any target/toolchain limitations.
- Do not assume documentation in adjacent repositories is current; prefer this repository's source, manifests, and CI configuration as ground truth.

# Optimization and Profiling

- For optimization requests, define the workload first and record a native baseline before editing hot paths. Use the same inputs, locale, disk state, resolution, network mode, duration and machine for the comparison. Build outside the measured interval and run `cargo make prepare-profiling` to warm application-build caches first.
- Build an optimized binary with symbols using `cargo make build-profiling --features prebuilt-applications` for OS workloads (omit the feature for independent benchmarks). Record uninstrumented measurements with `cargo run --manifest-path tools/profiling/Cargo.toml -- measure --scenario NAME --output target/profiles/BASELINE --runs 3 --duration 30 --applications-dir target/wasm32-wasip1/release-wasm -- EXECUTABLE ARGS...`. Read `summary.json` and per-run logs; a duration limit is a bounded observation, not successful workload completion. Do not use failed or forcibly killed runs to claim improvements.
- For attribution, rebuild with `cargo make build-profiling --features profiling,heaptrack`. The `profiling` feature records task/spawner/thread identity, cumulative poll CPU time, poll/wait wall time and allocation request volume in each run's `runtime.json`. The `heaptrack` feature annotates the native custom allocator. Features are disabled by default and Linux-only instrumentation compiles out on other targets.
- Use the recorder's `cpu` mode with separately installed Samply (or `--tool perf`) and `heap` mode with heaptrack. Read the README's profiling section for artifacts and limitations. Keep external profiler overhead and feature-enabled counters out of uninstrumented before/after consumption comparisons.
- CPU time, elapsed latency, RSS, allocator-used/reserved bytes and allocation request volume are different metrics. Task IDs can be reused: use the manager/generation identity in reports. Executor poll CPU includes task CPU; do not add them together. Snapshots are periodic, not final or peak heap reports. Check active poll/wait flags: long-running polls do not update completed CPU/counts until they return. Native improvements need follow-up validation on the intended Wasm/embedded target.
