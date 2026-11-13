# Region TLSF heap

`RegionHeap<FL = 16, SL = 8>` provides allocator mechanics, without owning platform
region setup, capabilities, critical-section locks, protection, or cache hooks.
Platforms' native, Wasm and Nordic managers wrap it. `Manager`, `ManagerTrait`,
`RegionStatistics`, `Statistics`, `Debug` and `Nightly` remain unchanged. The old
Core `region_allocator.rs` was entirely commented out and not compiled.

## Choice

Use [rlsf 0.2.3](https://docs.rs/rlsf/0.2.3/rlsf/) (MIT/Apache-2.0,
Rust 1.61, edition 2021), with defaults disabled. Its MSRV is below Core's 1.90
and it has releases in February and July 2026. It supports arbitrary alignment,
caller-supplied discontiguous pools, immediate coalescing, configurable FL/SL,
and grow/shrink reallocation attempts in place. Moving reallocations copy bytes
and therefore are not constant-time. The wrapper handles changed alignment by
allocating/copying/freeing, and leaves the original allocation live on failure.

The alternative [tlsf 1.1.0](https://docs.rs/tlsf/1.1.0/tlsf/) emphasizes smaller
metadata with compressed u16 pointers; its latest release is from 2024 and its
pool-size constraints are a poor fit for native/Wasm heaps. Writing another TLSF
would add unsafe splitting/coalescing and alignment code to maintain, with no
measured reason to expect a better result. rlsf supplies those mechanisms already.
No C implementation or FFI is required.

The exact version pin is intentional: accurate block statistics use rlsf's
`unstable` block iterator. Statistics walk blocks under the platform's existing
lock; allocation/free never walk them. This changes statistics queries to O(n),
but avoids per-allocation side tables, private-header access or extra headers.
`used` includes occupied block headers and alignment/rounding padding; `free`
includes free block headers. Totals exclude sentinels and alignment losses.
Thus used + free = usable total, and freeing everything returns used to zero.
These counters describe physical heap occupancy rather than requested bytes.
Statistics queries are not suitable for a hard real-time critical path.

## Parameters and RAM

Default FL=16, SL=8, first bitmap `usize`, second bitmap `u32`. SL is configurable
to powers of two up to 32; FL must fit the first bitmap. Granularity is fixed by
rlsf at four machine words (16 bytes on 32-bit, 32 on 64-bit). On 32-bit the
maximum block is 1 MiB minus 16 bytes; larger pools are partitioned. This covers
tens/hundreds of KiB heaps without limiting ordinary MCU allocations. More SL
bins cost more control RAM and reduce size-class search rounding.

Native and Wasm select FL=usize::BITS, SL=8, allowing rlsf to coalesce contiguous
extensions across previous pool sentinels. Smaller FL counts deliberately prevent
that coalescing (upstream splits at its maximum block size).

| Bytes | First-fit 32-bit / 64-bit | TLSF 32-bit / 64-bit |
|---|---:|---:|
| Raw allocator control, FL=16, SL=8 | 24 / 48 | 580 / 1096 |
| Core wrapper including pool span | n/a | 592 / 1120 |
| Live platform wrapper (Nordic / Wasm / native) | — | 592 / 1168 / 4384 |
| Allocated block header | 0 / 0 | 8 / 16 |
| Allocation size rounding | 4 / 8 | 16 / 32 |
| Minimum first-fit allocation | 8 / 16 | — |

TLSF additionally loses one granule per pool sentinel and at most granule-1 bytes
at an unaligned edge. Alignments >= granularity insert padding (up to alignment
minus half a granule); normal size rounding adds at most granule-1 bytes. Free
list links occupy free blocks rather than separate allocated metadata. Native's
larger fixed control is negligible relative to its 64 MiB initial mapping, but
MCU control size must be budgeted per region. No subsystem pools are reserved.

## Reproduction and results

From Core, set `CARGO_TARGET_DIR` to a directory under the workspace's
`target/work/`, then run:

```sh
cargo run -p xila-memory --release --example allocator_bench
```

The benchmark uses an aligned fixed 64/256 KiB backing store, 512 live slots,
xorshift seed 0x123456789abcdef, and 100,000 replacement iterations. Random slot
replacement gives mixed lifetimes; 31/32 requests are 16–256 bytes, 1/32 are
1–8 KiB; alignment is 8. Both allocators replay identical requests, including
requests that previously failed. Snapshot free space/largest allocatable payload
is measured before draining live objects. Largest payload is found by exhaustive
descending probes (no monotonic-fit assumption). Timings measure allocation and
free separately, including `Instant` overhead, excluding payload writes, probes,
statistics and workload generation. This is a synthetic UI workload, not an LVGL
trace or an end-to-end application benchmark.

Host: Linux x86_64, Intel i7-1370P, rustc 1.99.1, release defaults. Three raw runs
are in `measurements/`; concurrent builds affected later runs. Representative
run 1 (all space figures are bytes):

| Heap | Allocator | Failed requests | Total free | Largest payload |
|---|---|---:|---:|---:|
| 64 KiB | first-fit | 10102 | 7296 | 256 |
| 64 KiB | TLSF | 16179 | 4256 | 272 |
| 256 KiB | first-fit | 0 | 112320 | 74832 |
| 256 KiB | TLSF | 0 | 99968 | 40944 |

| Heap | Allocator | Alloc avg / p99 / max ns | Free avg / p99 / max ns |
|---|---|---:|---:|
| 64 KiB | first-fit | 104 / 208 / 3895 | 95 / 202 / 5469 |
| 64 KiB | TLSF | 28 / 50 / 4146 | 23 / 42 / 103100 |
| 256 KiB | first-fit | 106 / 230 / 4627 | 107 / 233 / 9508 |
| 256 KiB | TLSF | 28 / 50 / 4900 | 24 / 49 / 133 |

TLSF improves average and p99 allocator latency here, but **does not establish a
fragmentation improvement**. The smaller heap has 60% more failed requests; the
larger heap has a smaller contiguous remainder. Headers, granularity and different
placement matter. Scheduler outliers make observed maximum timings unsuitable
as WCET evidence; measure cycle counts on hardware for that. TLSF's constant-time
lookup does not bound memcpy, region selection, OS mapping or lock contention.

For Cortex-M code size, from Core:

```sh
export CARGO_TARGET_DIR="$PWD/../../target/work/allocator-size"
export RUSTFLAGS='-C link-arg=-e_start'
cargo build --manifest-path subsystems/memory/bench-size/Cargo.toml --target thumbv7em-none-eabihf --release
size "$CARGO_TARGET_DIR/thumbv7em-none-eabihf/release/xila-memory-allocator-size"
cargo build --manifest-path subsystems/memory/bench-size/Cargo.toml --target thumbv7em-none-eabihf --release --features first-fit
size "$CARGO_TARGET_DIR/thumbv7em-none-eabihf/release/xila-memory-allocator-size"
```

| Allocator | text bytes | data | bss |
|---|---:|---:|---:|
| First-fit 0.10.6 | 700 | 0 | 65536 |
| TLSF 0.2.3, 16 x 8 | 932 | 0 | 65536 |

This identical no_std alloc/free harness uses opt-level=s, LTO and one codegen
unit. Non-inlined externally visible allocation/free functions prevent specializing
the entire allocator to a single call. The +232-byte difference measures the
minimal allocator path, not a complete OS, statistics or reallocation. The
control structure is on the stack, so bss measures only the backing heap.

## Validation

Against Core base `82d756768deef2c01fa682d4fa675c11fd3ba301`:

- `cargo fmt --check`, `mise run format:toml -- --check`: pass.
- `cargo clippy -p xila-memory --all-targets -- -D warnings`: pass.
- `cargo test -p xila-memory`: 2 existing unit tests and 4 integration tests pass;
  3 pre-existing documentation examples remain ignored.
- `cargo check -p xila-memory --target thumbv7em-none-eabihf` and
  `--target wasm32-unknown-unknown`: pass.
- `mise run check:host`: pass on branch and an archived origin/main source tree.
- `mise run test:workspace`: branch and origin/main both stop at
  `real_vfs_and_c_client`, `FailedToSpawnNetworkTask(InvalidSpawnerIdentifier)`
  in `libraries/abi/application/c_tests/src/lib.rs:53`. The rest of the suite is
  not established by a command that stops there.
- `python3 -m unittest discover -s tools/ci`: all 52 pass, on branch and main.
  The package identity inventory includes the new standalone size harness.

All build and scratch outputs were placed under the enclosing workspace's
`target/work/`; build directories were removed after validation. Platforms'
separate migration includes native memory-manager tests and target builds.
