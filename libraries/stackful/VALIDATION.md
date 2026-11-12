# Validation record

Run from Core on an x86_64 Linux host, 2026-11-12. All scratch files and explicit
Cargo target directories were under the enclosing workspace's `target/work/`.
The branch started from origin/main d16cc5e6. Platforms was inspected read-only.
No application ABI code or workspace-repository commits were changed.

Passed:

* `npm ci` (once; completed with an engine-version warning for lv_font_conv).
* `mise trust` (configuration was already trusted).
* `cargo fmt --check`.
* `mise run format:toml -- --check`.
* `cargo clippy -p xila-stackful --all-targets -- -D warnings`.
* `cargo test -p xila-stackful`: one canary unit test and eight host integration tests.
* `cargo test --release -p xila-stackful`.
* `python3 -m unittest discover -s tools/ci`: 52 tests.
* `cargo check --all-targets --target x86_64-unknown-linux-gnu --features graphics,abi_definitions,graphics_rendering_xrgb8888`.
* `cargo check -p xila-stackful --target TARGET` and additionally `cargo build -p xila-stackful --target TARGET` for each:
  `thumbv7em-none-eabihf`, `thumbv6m-none-eabi`,
  `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf`,
  `riscv32imafc-unknown-none-elf`, `aarch64-unknown-linux-gnu`.
* `RUSTFLAGS='-C target-feature=+vfp4d16sp' cargo build -p xila-stackful --target thumbv7em-none-eabi --features fpu`.
  Rust warns this target feature is unstable; assembly generation succeeds.

Installed missing standard-library targets with:
`rustup target add thumbv6m-none-eabi thumbv8m.main-none-eabihf aarch64-unknown-linux-gnu`.
No target installation failed. Cross builds generate the assembly, whereas
`cargo check` alone would not verify its instruction encodings. They do not
establish hardware execution correctness. No ARM/RISC-V QEMU binaries were
available, so no emulated runtime tests were run.

Expected rejections:

* `cargo +esp check -p xila-stackful --target xtensa-esp32s3-none-elf -Z build-std=core,alloc --ignore-rust-version`:
  installed Espressif toolchain reaches the explicit windowed-ABI unsupported
  compile error. It is not an implemented Xtensa switch.
* `cargo check -p xila-stackful --target wasm32-unknown-unknown`: explicit unsupported compile error.
* Enabling `fpu` for thumbv7em-none-eabi without enabling hardware target
  instructions fails assembly generation, as intended; use the softfp command
  above or the hard-float target.

The first thumbv6m check exposed Core task's existing smol_str/Arc dependency
on pointer atomics. The final crate excludes that dependency there; the direct
Embassy coroutine driver and join pass check/build, while the task-manager
adapter remains unavailable until Core task supports that target.

Host measurements: Coroutine/JoinHandle 8 bytes each, Stack 24 bytes, shared
control allocation 136 bytes including Rc counts. Stack guard is 64 bytes;
rounding adds at most 15 bytes. Allocation regression asserts two allocations
at creation and zero during 100 suspend/resume cycles. Runtime task-manager
pool/metadata/join allocations are additional.

Optimized synthetic suspend/resume round trips measured 154–180 ns on this host
across two runs, including both switches, TLS, wrapper polling and guard scans.
These are development-host observations, not MCU latency claims. Disassembly
shows 51 bytes / 21 instructions for the x86_64 naked switch in one direction.
No corosensei linked-size comparison was attempted because its required
protected guard-page stack contract excludes the intended heap-only MCU stacks.
