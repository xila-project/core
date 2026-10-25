# Verification

Allocation-free, `no_std` cryptographic primitives. SHA-256 is enabled by default;
strict Ed25519 verification is optional. This library does not establish trust
policy or define a package format.

## API

```rust
# #[cfg(feature = "sha256")]
# {
use verification::ContentVerifier;

// Expected values must come from authenticated metadata, not the payload itself.
let expected = [
    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea,
    0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
    0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
    0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
];
let mut verifier = ContentVerifier::new(3, expected)?;
verifier.update(b"a")?;
verifier.update(b"bc")?;
verifier.verify()?;
# }
# Ok::<(), verification::Error>(())
```

`Sha256Verifier` also provides `new`, `update`, and consuming `finalize` for plain
digest calculation. SHA-256 accepts at most `2^61 - 1` logical bytes; declared
lengths above that limit fail at construction. Update errors permanently poison
the stream, even if ignored. Finalization rejects incomplete streams and consumes
state. Hashes have exactly 32 bytes by type, with no truncation or padding.
There is no success notification on drop and no heap/full-file buffer.

Enable `ed25519` explicitly to use `PublicKey::from_bytes(&[u8; 32])` followed by
`key.verify(message, &[u8; 64])`. Slice variants reject all incorrect lengths.
Key loading rejects noncanonical encodings, small-order and mixed-order points;
verification calls **Dalek `verify_strict` only**. This is ordinary Ed25519 over
the exact supplied bytes, **not Ed25519ph**. Signing a digest is not equivalent
to signing the original message. Callers bound and retain their message bytes;
no bespoke digest-statement schema or manifest encoding is provided here.

## Features and dependencies

| Feature | Behavior |
| --- | --- |
| `sha256` (default) | sha2 0.11 + subtle 2.6, defaults disabled |
| `ed25519` | ed25519-dalek 3.0, defaults disabled |
| neither | Error/Result types only |

SHA-256 and Ed25519 are independently selectable. Dalek uses SHA-512 from the
same sha2 0.11 dependency, not another implementation. This crate enables no
Dalek `alloc`, `batch`, `pem`, `legacy_compatibility`, `fast` or `zeroize` feature,
and exposes no signing operations. Cargo feature unification can change a final
application's dependency profile: audit **that application's** graph, not just
this crate. Host proc-macro/build dependencies using std do not imply runtime std.

The lockfiles record independently tested sha2 0.11.1 (Rust 1.88), Dalek 3.0.0
and curve25519-dalek 5.0.0 (Rust 1.85). The library minimum is Rust 1.90; a standalone
probe compiles with that compiler. The main workspace currently fails to parse
on Cargo 1.90 due to an unrelated `wasm` workspace-default-feature override.
Dalek/curve25519-dalek are BSD-3-Clause, sha2/digest MIT OR Apache-2.0,
subtle BSD-3-Clause. Mature dependencies do not constitute an independent audit
of this wrapper or every dependency version.
`cargo audit` found no known vulnerabilities in the standalone probe lockfile
on 2026-10-24 (1352-advisory RustSec database); this does not audit the rest of
Xila's workspace or prove that these dependencies are vulnerability-free.

## Existing Xila hash device

`drivers/shared/src/devices/hash.rs` already uses **sha2 0.11**. This library
reuses that implementation directly rather than introducing another SHA
algorithm or coupling to password authentication.

The current device allocates a `Box<dyn DynDigest>` and boxed private context
per open; algorithm changes allocate another digest. Reads clone the boxed
digest and finalize into an allocated digest output, preserving read-without-
reset behavior. Context cloning also allocates. The direct typed path avoids
those allocations, dynamic dispatch, context copies and VFS/runtime dependencies.
The device and its existing consumers are unchanged. A device adapter is not
needed for borrowed chunk input; changing its context representation would be
separate driver work with compatibility tests.

This is a **source audit**, not a measured end-to-end VFS benchmark. Total memory
(heap + live state + caller buffers + peak stack) matters, not allocator purity.
A future adapter or allocated workspace is acceptable if measured total-memory
evidence favors it. Zero allocations alone do not establish a small stack.

## Security / integration boundaries

- Caller-supplied keys must already be trusted. A key carried by an untrusted
  package is not trusted by parsing or verifying it. Signatures do not authorize
  device access or capabilities; rollback, freshness and key lifecycle are policy.
- #402 owns the single canonical signed representation. It must bind identity,
  version, paths/types, expanded sizes/hashes, target/ABI and security-relevant
  permissions/capabilities. This crate does not parse or validate those semantics.
- Feed the specified logical bytes, normally decompressed content. The parser
  bounds decompression and framing; the installer writes only quarantined staged
  files. Any read, write, cancellation or verification failure must abort before
  activation or an install hook—even if hashing itself succeeds.
- #491 owns installation/trust/recovery. #495 owns loader enforcement against
  authenticated installed metadata and TOCTOU-safe execution of those same bytes.
  No CLI subprocess is an enforcement boundary. Neither integration ships here.

## Reproduced evidence (2026-10-24)

Compiler: rustc 1.99.1; Linux x86_64, AMD Ryzen 9 3900X. Library tests cover
published NIST/FIPS SHA-256 examples and RFC 8032 §7.1 tests 1/2, mutation,
length/truncation/poisoning and adversarial key/scalar cases. These are not a
claim of a full Wycheproof run. The counting allocator detects a positive
allocation control and observes zero allocations for successful and rejected
verification paths, including 4 MiB streamed content.

| Native state (not peak stack) | Bytes |
| --- | ---: |
| Incremental SHA-256, including checked counter/status | 120 |
| Length + expected digest content verifier | 160 |
| Expanded validated public key | 192 |

Raw trusted keys can stay in Flash and be validated on demand to trade CPU/stack
for retained RAM. Caller input buffers are additional; the example uses 512 bytes.
Native release medians: SHA-256 64 KiB 32.71 µs, 1 MiB 522.38 µs, 4 MiB
2483.89 µs; valid 1 KiB Ed25519 49.32 µs; key validation 43.97 µs. Hash throughput
is approximately 1610–1914 MiB/s on this SHA-instruction-capable CPU, with no I/O.
These numbers are not MCU latency estimates.

No-allocator **final links** pass for Cortex-M4 `thumbv7em-none-eabi`, browser
WASM `wasm32-unknown-unknown`, and native x86_64; the allocator-using positive
control fails as intended. Four feature combinations pass native tests/Clippy
and compile checks on Cortex-M4, browser WASM and WASI. WASM checks/links are
not browser execution tests.

Linked Cortex-M4 probe totals (`llvm-size` text = code + read-only metadata,
`opt-level=z`, LTO, panic abort; no board runtime):

| Profile | Default SHA backend bytes | Compact software SHA bytes |
| --- | ---: | ---: |
| Empty baseline | 42 | 42 |
| SHA-256 | 9218 | 2510 |
| Strict Ed25519 + key validation | 43018 | 17284 |
| Both | 51252 | 18828 |
| Both + Dalek fast | 58892 | 26468 |

All Cortex-M probe profiles have zero `.data` / `.bss`. These are entire linked
probe totals; subtract the empty baseline for a profile delta, not an exact
per-function attribution. Compact SHA substantially lowers code size; fast adds
7640 bytes here. Neither option is imposed on firmware. See
`tools/verification_probe/README.md` for exact reproducer commands and native
size probes; build configuration and caller code change the result.

**Still unmeasured / integration gates:** actual MCU execution, peak stack
(including callers, runtime and interrupts), peak total RAM and target latency;
ESP32/Xtensa support and target-specific firmware Flash cost; runtime loader and
installer enforcement. No board was available. In particular, zero heap/static
RAM is not zero working memory, and no safe task-stack budget is claimed. Keep
#490 open and firmware integration gated on these results and #402's signed-byte
definition. Do not inherit any measurements from the unavailable prototype.

## Validation commands

```sh
bash libraries/verification/check.sh
cargo tree -p verification --all-features -e features
bash tools/verification_probe/check.sh thumbv7em-none-eabi
COMPACT_SHA2=1 bash tools/verification_probe/check.sh thumbv7em-none-eabi
```

The focused matrix expects installed Cortex-M/WASM targets; override
`VERIFICATION_TARGETS` for a narrower check and report that restriction.
CI runs the full matrix and no-allocator Cortex-M/WASM link probes through
`mise run ci:verification`. No firmware or full package workflow is enabled.
