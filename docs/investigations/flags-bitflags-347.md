# #347 — Custom `flags!` versus `bitflags` 2

Status: **breaking API changes are permitted; prefer idiomatic Rust and removal of compatibility shims.**

## Current inventory (default branch)

The custom macro is defined in `modules/shared/src/flags.rs`. The following direct invocation sites were found in GitHub code search:

| Subsystem | File | Types |
| --- | --- | --- |
| Memory | `modules/memory/src/capability.rs` | `CapabilityFlags` |
| Network | `modules/network/src/fundamentals/dns.rs` | `DnsQueryKind` |
| Filesystem | `modules/file_system/src/fundamentals/flags.rs` | `AccessFlags`, `CreateFlags`, `StateFlags` |
| Filesystem | `modules/file_system/src/fundamentals/control.rs` | `ControlDirectionFlags` |
| Filesystem | `modules/file_system/src/fundamentals/permission.rs` | `Permission`, `Special` |
| Shared | `modules/shared/src/flags.rs` | test types |

This is an initial search inventory, not a verified exhaustive call graph.

## Compatibility requirements

- Keep underlying `u8`/`u16` representations, `repr(transparent)`, values, and alignment.
- Preserve `None`, `All`, `bits_used()`, and public named constants.
- Preserve the *value-returning* `insert`, `remove`, `toggle`, and `set` methods used in const expressions. The `bitflags` API is not a drop-in replacement for these signatures.
- Preserve `from_bits` strict validation, truncation, and unchecked/raw construction semantics.
- Preserve `Debug` output where observable.
- **Important:** current `!flag` complements the entire storage word, including undefined bits. The `bitflags` `Not` implementation masks to known bits; migration requires a deliberate compatibility decision.
- Filesystem `Flags` packs values into `u16` using `bits_used()` and accesses tuple fields directly. Permission packing similarly accesses `.0` and constructs tuple structs. Any replacement must adapt these call sites without changing the on-disk/ABI bit encoding.
- `ControlCommandIdentifier` decodes its direction via raw tuple construction; preserve encoding exactly.

## Prototype plan

1. Add `bitflags = { version = "2", default-features = false }` **only on an experimental branch**.
2. Prototype one simple type and one packed filesystem type, with compatibility shims if necessary. Keep the existing macro for the remaining types.
3. Run the baseline tests added in PR #437, plus golden-value tests for packed `Flags`, `Permissions`, and control identifiers.
4. Compare `cargo test -p shared`, native workspace checks, WASM checks, and ESP32-S3 builds using the repository's supported build commands.
5. Record reproducible release binary sizes (especially flash), build times, and source LOC on the same toolchain, features, and target. Run more than once for timing comparisons.
6. Decide go/no-go. **Do not replace the macro globally until the evidence is collected.**

## Preliminary assessment

The custom macro is more than a flags declaration helper: it defines an API and storage contract. Source LOC reduction alone cannot justify migration. There is currently **no verified binary-size or compile-time result**, so the decision is **pending measurements**.

## Updated project direction (2026-10-19)

The maintainer explicitly permits breaking changes and prioritizes code cleanliness. Accordingly, the eventual migration should use the idiomatic `bitflags` 2 API rather than preserving Xila-specific naming and method signatures. Remove the custom macro and its redundant helper methods once migration is validated. Update all call sites to `empty()`, `all()`, `bits()`, `from_bits_retain()`, and normal mutable flag operations. Avoid compatibility aliases or wrappers solely to retain old source-level behavior.

Breaking **source API** compatibility is acceptable; changes to persistent filesystem encodings, hardware protocol fields, and ABI used across separately built components must still be explicit and tested. The existing regression tests describe legacy behavior, not necessarily required future behavior; revise tests for intentionally changed semantics (notably `Not` and `Debug`) rather than constraining the new implementation to old quirks.

The initial issue's investigation/measurement gate remains in place until a measured go/no-go decision, unless explicitly amended by the maintainer.

## Implementation status

The migration is complete: all seven production declarations (memory, network, and the five
filesystem types) use `bitflags` 2 with `default-features = false`, every identified call site
uses the idiomatic API, and `modules/shared/src/flags.rs` is deleted. The filesystem packed
wrappers (`Flags` and `Permissions`) intentionally remain custom tuple structs: they encode
multiple independent bit fields and are not plain flag sets. The `origin/main` commits merged
into this branch introduced further `AttributeFlags::All`/`None` call sites; they were migrated
as part of the merge.

## Validation (2026-10-19, local host, rustc 1.99.0)

| Check | Result |
| --- | --- |
| `cargo make format-rust/toml/json` (`--check`) | pass |
| `cargo make check-host` / `check-guest` | pass |
| `cargo make clippy-host` / `clippy-guest` | pass |
| `cargo test -p file_system -p memory -p network` (206 / 2 / 15 tests) | pass |
| `cargo metadata --locked` | pass |
| `cargo make test` (full workspace), `cargo make check-localization` | **not run locally**: the host lacks `pkg-config` and ALSA headers (`alsa-sys`); delegated to CI, which runs in the development-environment image |
| Native/WASM/ESP32 release builds, flash size | **not measured** (see measurements below) |

Issues found and fixed while finishing the draft:

- `Cargo.lock` referenced the unqualified name `bitflags` for `file_system`, `memory`, and
  `network` while the graph contains both 1.3.2 and 2.13.2; cargo rewrote those entries on
  every resolve, failing all `--locked` jobs (Localization (fr), WASM ABI). The lock is now
  regenerated by cargo.
- Three files were not rustfmt-formatted (Format checks).
- `Permissions` is a plain tuple struct, not a `bitflags` type: two migrated call sites used
  `from_bits_retain`/`bits()` on it and did not compile.
- `modules/abi/definitions/src/memory/allocation.rs` still used `CapabilityFlags::None`.

## Measurements

- **Source LOC (vs `origin/main`):** net −274 lines over 29 files (Rust-only −336); the deleted
  `modules/shared/src/flags.rs` alone accounts for 464 removed lines. Added parity tests offset
  part of the removal.
- **Build time:** warm `cargo make check-host` on the branch takes 1.43–3.16 s over three runs
  (same machine, same toolchain). **The before/after comparison is not yet valid**: this
  machine's global `~/.cargo/config.toml` shares one `target-dir` across all checkouts, and
  alternating between this branch and a baseline checkout invalidated the baseline cache during
  measurement. A sound comparison requires isolated `CARGO_TARGET_DIR`s per checkout and runs
  alternating on an otherwise idle machine — pending.
- **Flash / binary size:** not measured. No ESP32 toolchain is available locally and this
  repository's CI defines no ESP32 job, so flash impact cannot be established here; native or
  WASM release binary sizes can stand in until a hardware toolchain is available.

## Semantic changes versus the legacy macro (intentional)

- `Not` now masks to defined bits; the legacy macro complemented the entire storage word
  including undefined bits.
- `Debug` uses `bitflags` formatting instead of `Type { A | B }`.
- No compatibility shims: no `None`, `All`, `bits_used()`, or value-returning
  `insert`/`remove`/`toggle`/`set`; call sites use `empty()`, `all()`, `union()`, `intersection`,
  `difference`, `bits()`, `from_bits_retain()` and mutable operations.
- Persistent encodings are unchanged and covered by golden-value tests: flag bit values for all
  seven types, unknown-bit handling (`from_bits` rejects, `from_bits_truncate` drops,
  `from_bits_retain` keeps), the `u16` packing of `Flags` (mode bits 0-1, open bits 2-4, status
  bits 5-8), `Permissions` octal layout, and `ControlCommandIdentifier` raw encoding.
