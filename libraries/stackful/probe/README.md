# Experimental corosensei measurements

Run from the Core worktree root. The probe depends on the `xtensa-support`
branch of the corosensei fork (corosensei 0.3.4 plus an Xtensa backend,
pinned by revision in `Cargo.toml`), with a small local measurement patch on
top (`corosensei-xtensa-fork.patch`). The patch permits a private,
caller-audited 2 KiB stack and spells out the Cortex-M FP register clobbers.
Use it only to reproduce the measurements. The stack-size safety obligation
covers the entry, all callees and any interrupt frames on the same SP.
Corosensei 0.3.4 also imposes a 1024-byte cap on inline entry captures.

```sh
mkdir -p target/probe target/scratch
export TMPDIR="$PWD/target/scratch"
export CARGO_TARGET_DIR="$PWD/target/probe-build"
git clone https://github.com/AlixANNERAUD/corosensei.git target/probe/corosensei
git -C target/probe/corosensei checkout 4d0a022e99728e3b028af4b1497e2d926cdb9d2d
git -C target/probe/corosensei apply "$PWD/libraries/stackful/probe/corosensei-xtensa-fork.patch"
patch_config="patch.\"https://github.com/AlixANNERAUD/corosensei.git\".corosensei.path=\"$PWD/target/probe/corosensei\""
export RUSTFLAGS="--cfg stackful_corosensei_probe"
cargo +1.97.0 --config "$patch_config" test -p xila-stackful --features corosensei-probe \
    -p xila-abi-application -p xila-abi-application-c-tests -p xila-virtual-file-system
cargo +1.97.0 --config "$patch_config" test --release -p xila-stackful \
    --features corosensei-probe footprint_and_switch_timing -- --exact --nocapture
```

A patched run may rewrite the corosensei entry in `Cargo.lock` to a path
source. Restore the Git source with `git restore -p Cargo.lock` or by running
any unpatched Cargo command before committing. Pass `--config` after the
subcommand for `cargo clippy`, which otherwise drops it.

To build the firmware probe, install the targets with `rustup target add
--toolchain 1.97.0`. Run the following for `thumbv7em-none-eabihf` with
`layout=arm` and `riscv32imac-unknown-none-elf` with `layout=riscv`.

```sh
probe="$PWD/libraries/stackful/probe"
cp Cargo.lock "$probe/Cargo.lock"   # ignored by Git
export RUSTFLAGS="--cfg stackful_corosensei_probe --remap-path-prefix=$PWD=/core -C link-arg=-T$probe/$layout.ld"
cargo +1.97.0 --config "$patch_config" build --manifest-path "$probe/Cargo.toml" \
    --features stackful/corosensei-probe --release --target "$target"
elf="$CARGO_TARGET_DIR/$target/release/stackful-firmware-probe"
llvm-size -A "$elf"
llvm-size -A "$CARGO_TARGET_DIR/$target/release/deps/"libstackful-*.rlib
```

Sum `.text` and `.text.*`, and separately `.rodata` and `.rodata.*` for the
rlib. Use a fresh Cargo target directory to select a single rlib. The rlib
contains only Xila's emitted objects. The ELF includes the instantiated
generics, corosensei and the rest of the reachable firmware. To measure the
baseline, copy this probe directory to a checkout of the base commit, use its
Core lockfile, omit the patch configuration, feature, and experimental cfg, and
use a fresh target directory with the same toolchain and release profile.

Run the ARM ELF with `qemu-system-arm -M mps2-an386 -nographic
-semihosting-config enable=on,target=native -kernel "$elf"`. Run the RISC-V ELF
with `qemu-system-riscv32 -M virt -nographic -bios none -kernel "$elf"`.
Each firmware spawns one Xila coroutine with a 2 KiB usable stack. The coroutine
yields once via `suspend_on` and returns 42. The firmware also prints the shared
control allocation and the deepest paint byte changed by that workload. Paint
does not account for untouched reserved stack, interrupt frames or larger call
graphs. An optional `fp-register` probe feature checks a callee-saved FP register
on both sides of the switch on Cortex-M hard-float and RV32IMAFC. Use the
feature only for the FP check, since it changes the firmware under measurement.

## Xtensa (ESP32, ESP32-S2, ESP32-S3)

Corosensei has no Xtensa backend upstream (Amanieu/corosensei#79); the fork's
backend needs the esp-rs toolchain (`espup install`, channel `+esp`, Rust 1.97
base) and `-Zbuild-std`. The library builds for all three chips:

```sh
export RUSTFLAGS="--cfg stackful_corosensei_probe"
for chip in esp32 esp32s3; do
    cargo +esp --config "$patch_config" build -p xila-stackful --features corosensei-probe \
        --target xtensa-$chip-none-elf -Zbuild-std=core,alloc --release
done
# The ESP32-S2 has no atomic CAS: xila-task's portable-atomic needs this cfg.
RUSTFLAGS="$RUSTFLAGS --cfg portable_atomic_unsafe_assume_single_core" \
    cargo +esp --config "$patch_config" build -p xila-stackful --features corosensei-probe \
        --target xtensa-esp32s2-none-elf -Zbuild-std=core,alloc --release
```

`probe/xtensa` is a firmware for Espressif's QEMU fork (`esp32`, `esp32s3`
machines; none exists for the ESP32-S2, which is build-only). It uses
esp-hal 1.0, which Espressif's QEMU can boot, and prints its results on UART0.
With `--features scenarios` it first runs the stackful runtime scenarios
(interleaving, nested calls, call chains deeper than the 16 register windows,
cancellation, allocation budget, a 2 KiB call chain, TLS slots, FP state, nested
coroutines and painted-canary detection), then the same one-coroutine footprint
workload as the other probes. The run ends with `PROBE OK`; a `baseline`
feature boots without any coroutine, to measure the firmware delta.

```sh
export PATH="$HOME/.cargo/bin:$(echo ~/.rustup/toolchains/esp/xtensa-esp-elf/*/xtensa-esp-elf/bin):$PATH"
chip=esp32s3   # or esp32
export CARGO_TARGET_DIR="$PWD/target/xtensa-probe"
export RUSTFLAGS="--cfg stackful_corosensei_probe -C link-arg=-nostartfiles -C link-arg=-Tlinkall.x -C link-arg=-Wl,--no-warn-rwx-segments"
cargo +esp --config "$patch_config" build --manifest-path libraries/stackful/probe/xtensa/Cargo.toml \
    --release --target xtensa-$chip-none-elf -Zbuild-std=core,alloc --features $chip,scenarios
elf="$CARGO_TARGET_DIR/xtensa-$chip-none-elf/release/stackful-xtensa-probe"
espflash save-image --chip $chip --merge --flash-size 4mb "$elf" "$CARGO_TARGET_DIR/flash.bin"
qemu-system-xtensa -nographic -machine $chip \
    -drive file="$CARGO_TARGET_DIR/flash.bin",if=mtd,format=raw -serial file:probe.log -monitor none
```

`qemu-system-xtensa` comes from <https://github.com/espressif/qemu/releases>.
Xtensa has no Xila baseline to compare with, so flash is the difference between
the `baseline` and full firmware. CCOUNT ticks under QEMU are not cycle counts.
`libraries/stackful/probe/xtensa/Cargo.lock` is ignored by Git, like the
other probe's lockfile.
