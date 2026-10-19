# #347 — Custom `flags!` versus `bitflags` 2

Status: **investigation only — no migration approved**.

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
