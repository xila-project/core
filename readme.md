# Xila — Rust embedded operating system with WebAssembly apps

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/xila-horizontal-white.svg">
  <img src="assets/brand/xila-horizontal-ink.svg" alt="Xila" width="300">
</picture>

Build embedded experiences with **Xila**, a modular, open-source operating system
written in Rust. Xila brings together WebAssembly (Wasm) applications, an LVGL
graphical interface, task management, networking, and storage for
resource-constrained devices.

[Documentation](https://www.xila.dev/docs/) ·
[Run Xila natively](https://www.xila.dev/tutorials/native) ·
[Try the browser example](https://www.xila.dev/tutorials/browser) ·
[Community](https://matrix.to/#/#xila:anneraud.fr)

[![codecov](https://codecov.io/github/Xila-Project/Core/graph/badge.svg?token=01M6GQFBEC)](https://codecov.io/github/Xila-Project/Core)
[![Translation status](https://translate.anneraud.fr/widget/xila-core/svg-badge.svg)](https://translate.anneraud.fr/engage/xila-core/)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

## What you can explore

- **Wasm applications:** run guest programs with the Wasmi runtime and explore
  the included Calculator and Weather apps.
- **Graphical interfaces:** build on LVGL, the shared Shadcn theme, and Xila's
  graphical shell.
- **OS building blocks:** work with task scheduling, a virtual filesystem,
  device drivers, and networking through portable Rust crates.
- **Native development:** develop and investigate CPU and memory use on Linux
  before validating shared code on your intended embedded target.

Xila is under active development. Hardware support varies by target; native and
browser examples provide development environments, not proof that a particular
board is supported. See the [capabilities and limits](https://www.xila.dev/reference/)
before choosing a target.

## Start here

New to Xila? Follow the [native tutorial](https://www.xila.dev/tutorials/native)
for a guided first run, or the [browser tutorial](https://www.xila.dev/tutorials/browser)
to build a Wasm guest and run Calculator in browser-hosted Xila. To create an
application, start with [building a guest](https://www.xila.dev/how-to/build-guest).

This repository contains the OS implementation. The
[website repository](https://github.com/xila-project/website) contains the
documentation and learning guides.

## Build and check Core

Install [mise](https://mise.jdx.dev/getting-started.html). By default, this
repository selects the `development` tasks. Tools are installed on demand per
task; optionally preinstall all development task tools in one pass:

```bash
mise trust
mise install --include-task-tools # optional: preinstall development task tools
mise bootstrap packages apply --manager apt
mise run deps:npm
mise run check:all               # Linux host + WASM guest checks
```

No development container is required. `.miserc.toml` selects
`mise.development.toml` by default; tasks select host or WASM targets, formatting
or coverage tools as needed, including developer-only `format:*`,
`translate:*` and `dev:*` tasks. `mise.toml` holds reusable namespaced tasks
such as `check:host`, `docs:all`, and `image:wasm`.
CI selects `mise.ci.toml`, where tools are declared on individual
tasks so a job installs only the tools its tasks use. On Debian/Ubuntu, native
packages are provisioned from the selected environment with
`mise bootstrap packages apply --manager apt` (mise requests sudo when needed).
Developer Cargo build tasks are wrapped by mbx through Mise's native Rust
integration. Tools are not activated globally: use `mise run build:cargo -- ...`
or `mise run run:cargo -- ...` for ad-hoc Cargo commands with this setup.
CI uses the mbx GitHub Action and its wrapper instead, avoiding a second mbx
installation in CI. Formatting tasks select only rustfmt, Taplo or Node, with
no build targets, Clippy, mbx, coverage tools or native-package bootstrap.

Other checks:

```bash
mise run clippy:all
mise run format:rust -- --check
mise run format:toml -- --check
MISE_ENV=ci mise run ci:quality-js
```

For opt-in native PCM playback using nanoaudio, see
[the audio library](libraries/audio/README.md). Run its tone example with
`mise run run:cargo -- -p xila-drivers-native --features audio --example audio`.

For GUI builds from a **Wayland desktop**, keep the host GPU drivers and check
rendering, input and window closing. Windows requires x86_64 WSL2/WSLg and a GUI
smoke test. Native Windows, macOS, ARM Linux and NixOS are unvalidated; the
current host checks and native event loop depend on Linux/Wayland.

For the browser example:

```bash
mise run build:applications
MISE_ENV=ci mise run pages:demo
```

`pages:demo` builds the browser example with its task-scoped nightly toolchain
and Trunk. Serve `examples/wasm/dist` separately to test rendering/input.

Tool versions are centralized in `[vars]` in `mise.toml`; npm dependencies are pinned by
`package-lock.json`. Local task names are listed by `mise tasks` and run with
`mise run <task>`. Tool pins do not freeze untracked Cargo lockfiles or moving
Git dependencies.

ESP-IDF, flashing and serial permissions need separate setup: follow the esp-rs
[Xtensa](https://docs.esp-rs.org/book/installation/riscv-and-xtensa.html) or
[RISC-V](https://docs.esp-rs.org/book/installation/riscv.html) guides. Host/WASM
checks do not prove ESP32, nRF, STM32 or other hardware correctness.

## Localization

Xila embeds each language at compile time and tracks per-crate translations with
[Tolgee](https://translate.anneraud.fr/engage/xila-core/). English (`en`)
is the fallback, with French (`fr`), Spanish (`es`), German (`de`), Brazilian Portuguese (`pt-BR`), Italian (`it`) and Polish (`pl`) catalogs. Build
the host, Wasm applications, fonts and seed image with the **same**
`EMBEDDED_I18N_LOCALE` value.

Simplified Chinese (`zh-CN`) catalogs are available for validation, with
catalog-subsetted Source Han Sans SC glyphs alongside Montserrat. Native-speaker
and embedded-target review are still pending; Chinese is not yet enabled for
deployment. Japanese (`ja`) also has experimental catalogs and
catalog-subsetted JP-region Source Han Sans glyphs. The Japanese build
requires an explicitly installed source font; neither CJK locale is enabled
for release/deployment until native-speaker and device review. See the
[CJK font and validation notes](subsystems/graphics/fonts/README.md).

See [localized build and contribution instructions](docs/localization.md) for
validation commands, translation review and artifact compatibility.

## Graphics theme

Graphics uses [lvgl_theme_shadcn](https://github.com/AlixANNERAUD/lvgl_theme_shadcn)
with the Zinc dark palette and Xila's generated fonts. The theme compiles alongside
LVGL using the same configuration and custom allocator. `Manager::update_theme`
updates the shared palette, including existing widgets, under the LVGL lock.

Prefer `graphics::theme::set_surface` and LVGL's Shadcn variant/error helpers over
local theme colors. Keep layout, transparent overlays, application branding and
specialized typography local where needed. Theme helpers require the graphics
lock, just like other LVGL calls.

## Native profiling

Profiling is intended for developers and optimization agents on Linux. It is
opt-in and adds no hooks/counters to default builds. The recorder and its checks
use the existing Rust toolchain; install [Samply](https://github.com/mstange/samply), Linux `perf`,
or [heaptrack](https://github.com/KDE/heaptrack) separately when needed. The
runner never installs tools or changes kernel permissions. Linux CPU recording
requires access to perf events; a permission error is reported in the run log.

Build before recording, after mise has installed the project tools:

```bash
mise run image:native
mise run build:profiling -- --features prebuilt-applications
cargo run --manifest-path tools/profiling/Cargo.toml -- measure \
  --scenario terminal-idle --output target/profiles/before --runs 3 --duration 30 \
  --applications-dir target/wasm32-wasip1/release-wasm \
  -- target/profiling/native_example --terminal --no-network --seed-image target/seed/native/seed.img
```

The `profiling` Cargo profile keeps release optimizations and full debug symbols.
If using an explicit Cargo target or custom target directory, adjust the binary
path. Every native build loads bundled applications from the disk. Prepare the
reference image before measurement and pass `--seed-image` for a new disk;
existing disks are mounted without reinstallation. The `prebuilt-applications`
feature is retained for compatibility. `--applications-dir` and
`XILA_PROFILE_APP_DIR` let the recorder identify and hash guest build artifacts;
they no longer cause runtime compilation or copying. See the
[native example](examples/native/README.md) and [image builder](tools/fs_image/README.md).
Measurements still include OS startup; they are not steady-state-only data.
Use the same disk fixture and settings on each run. A mutable disk can change
between runs; restore its baseline if the workload writes to it.

For allocation and task attribution, build with the optional features:

```bash
mise run build:profiling -- --features profiling,heaptrack
cargo run --manifest-path tools/profiling/Cargo.toml -- cpu \
  --scenario gui-navigation --output target/profiles/cpu --runs 1 --duration 60 \
  --applications-dir target/wasm32-wasip1/release-wasm \
  -- target/profiling/native_example --gui --no-network --resolution 800x600 --seed-image target/seed/native/seed.img
cargo run --manifest-path tools/profiling/Cargo.toml -- heap \
  --scenario gui-navigation --output target/profiles/heap --runs 1 --duration 60 \
  --applications-dir target/wasm32-wasip1/release-wasm \
  -- target/profiling/native_example --gui --no-network --resolution 800x600 --seed-image target/seed/native/seed.img
```

Replay the same manual GUI interactions for exploratory recordings. For repeatable
comparisons, use a bounded scripted workload and pass `--stdin INPUT_FILE` when
it accepts stdin; the file is replayed for every run. The default stdin is empty.
The runner also works with an already-built benchmark or test harness, so agents
can isolate a hot path without starting the whole graphical OS.

Each new output directory contains `summary.json`, per-run stdout/stderr logs,
and any profiles. Existing directories are refused to protect previous results.
The summary retains exact argv, executable hash, Git revision/dirty status,
selected build/locale settings, prebuilt application hashes, CPU model, duration, exit status and median
CPU/RSS measurements. It does not copy the full environment or credentials.
A duration limit stops the process group; forced kills and failed runs have no
successful median. Inspect the logs to establish that the intended scenario
actually ran. Profiler recordings must produce a nonempty artifact.

With the `profiling` feature, `runtime.json` is replaced atomically about once a
second. It records allocator-used/reserved bytes, task names/IDs/generations,
spawners/OS threads, poll counts and CPU time, maximum poll wall time, executor
poll/wait totals, and successful allocation request count/volume per task.
Completed task generations remain in memory until exit. Snapshots can lag exit
and are not final or peak heap measurements; counter updates during a snapshot
are not a transaction. In-progress polls/waits are counted only when they return. Tasks not spawned through the Xila manager are absent.
Executor CPU includes task CPU; do not sum both. Poll CPU uses OS-thread CPU
time, while wall time can include blocking/descheduling. Allocation volume
counts requested bytes, not retained memory or ownership of later frees.
Profiling metadata and the reporter itself add overhead in feature-enabled runs.

- Samply profiles: `samply load target/profiles/cpu/run-1/cpu.json.gz`.
- Perf alternative: add `--tool perf` to CPU recording; inspect
  `cpu.perf.data` with `perf report` or Hotspot.
- Heap reports: open the generated `heaptrack*` data file with `heaptrack_gui`
  or `heaptrack_print`. Xila's custom `mmap` allocator needs the `heaptrack`
  feature; plain malloc interception misses its suballocations. Reallocation is
  reported as the allocator's actual allocate/copy/free operations. Requested
  sizes exclude allocator padding and Xila ABI allocation headers are included
  when the manager receives them. Heaptrack also captures ordinary libc calls.

For before/after consumption comparisons, rebuild **without** `profiling` and
`heaptrack` (retain only `prebuilt-applications` for the OS workload), repeat `measure` with the same scenario/inputs, and compare medians
and individual runs. Linux `wait4` CPU accounting includes waited-for children;
peak RSS is a high-water mark, not a sum of simultaneous process-tree memory.
Instrumented CPU/heap runs explain costs; their timings are not uninstrumented
benchmarks. No heavy profiler is run automatically on PRs or merge groups.
Native findings guide shared-code optimization, but validate gains on the actual
Wasm/embedded target before claiming target-specific improvements.

Local checks:

```bash
cargo test --manifest-path tools/profiling/Cargo.toml
cargo test -p xila-task -p xila-drivers-std --lib --features task/test_harness,drivers_std/profiling,drivers_std/heaptrack
cargo test -p xila-native-example --bin native_example --features profiling,heaptrack
cargo run --manifest-path tools/profiling/Cargo.toml -- heap-smoke
```

The heaptrack smoke test compiles a local C API stand-in and verifies real native
allocation events, failed requests, realloc preservation and silence without the
feature. It does not require installing heaptrack or access to perf events.

## Find your way around

| Directory                      | Contents                                                                     |
| ------------------------------ | ---------------------------------------------------------------------------- |
| [`subsystems/`](subsystems/)   | Runtime-owned services: tasks, VFS, networking, graphics, memory, and time.   |
| [`libraries/`](libraries/)     | Reusable contracts, algorithms, adapters, and concrete implementations.      |
| [`drivers/`](drivers/)         | Platform support and device drivers.                                         |
| [`executables/`](executables/) | Shells, applications, and the Wasm runtime and guest bindings.               |
| [`examples/`](examples/)       | Native and browser integration examples.                                     |
| [`tools/`](tools/)             | Development tools, including native profiling.                               |

The [component map](https://www.xila.dev/explanation/architecture/components)
connects the source tree to architecture explanations and API references.

## Contribute

Start with the [contributor guide](https://www.xila.dev/how-to/contribute-core)
and [AGENTS.md](AGENTS.md) for development conventions. Browse
[open issues](https://github.com/xila-project/core/issues) for planned work,
report bugs with reproduction steps, or help improve
[translations](https://translate.anneraud.fr/engage/xila-core/).

Join the [Xila Matrix room](https://matrix.to/#/#xila:anneraud.fr) to discuss
ideas or ask for help.

## License

This project is under the [Apache License 2.0](license.md).
