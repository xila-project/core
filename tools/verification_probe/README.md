# Verifier link and size probes

This standalone workspace links `verification` without `std`, `alloc` or a
global allocator. It retains runtime inputs for hashing and strict signature
verification. The exported unsafe `probe` is **not runnable firmware**: Cortex-M
has no vector table, board memory map, initialization or interrupts. ELF section
sizes are linked algorithm/probe footprints, not complete firmware Flash budgets.

Install the targets using the compiler that runs Cargo:

```sh
rustup target add thumbv7em-none-eabi wasm32-unknown-unknown wasm32-wasip1
bash libraries/verification/check.sh
bash tools/verification_probe/check.sh thumbv7em-none-eabi
COMPACT_SHA2=1 bash tools/verification_probe/check.sh thumbv7em-none-eabi
bash tools/verification_probe/check.sh wasm32-unknown-unknown
bash tools/verification_probe/check.sh x86_64-unknown-linux-gnu
```

The script final-links empty, SHA-256, Ed25519, combined and combined+Dalek-fast
profiles, then checks that an allocation-using positive control fails. Target
tools are not installed by the scripts. `llvm-size`, when available, prints ELF
allocated section totals. Inspect `.text`, `.rodata`, `.data`, `.bss` separately
with `llvm-size -A` / `llvm-readelf -S`; the Berkeley `text` total includes read-only
tables and metadata, not instructions alone. Native PIE/linker padding and CPU
dispatch differ from Cortex-M; do not use native BSS as verifier working RAM.
Output is under `target/verification-probe` (or `$CARGO_TARGET_DIR`). Compact and
default runs reuse the same artifact paths, so retain logs before the next run.

`COMPACT_SHA2=1` adds only probe-target rustflags:
`--cfg sha2_backend="soft" --cfg sha2_backend_soft="compact"`.
The core library never sets backend flags. `fast` exists only on this measurement
tool and enables Dalek's precomputed tables; it is not enabled in the library.
Probe release settings are `opt-level=z`, LTO, one codegen unit, panic abort.
The probe deliberately exposes pointers so the compiler cannot replace verification
with a known result; callers must uphold the documented safety contract.

Native latency/throughput (build before timing):

```sh
cargo build --locked -p verification --example resources --release --all-features
target/release/examples/resources
```

This host-only example uses a public test seed to create its 1 KiB signature
fixture outside timing. It does not ship signing keys in firmware. It reports
31-sample hashing medians over 512-byte chunks and 301-sample key/verification
medians. OS scheduling, CPU frequency and load affect results; rerun on the same
machine for comparisons. It measures hashing without I/O/copying and verifies
with an already validated key. Key validation is timed separately.

To check the library's Rust 1.90 minimum independently of unrelated workspace
manifests, run:

```sh
cargo +1.90.0 check --locked --release --manifest-path tools/verification_probe/Cargo.toml --features sha256,ed25519
```

This is a compiler check, not a link proof or a test suite on Rust 1.90.
