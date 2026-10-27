# Experimental firmware update reference

This implements the research/native-prototype portion of [#526](https://github.com/xila-project/core/issues/526). **It is not a production bootloader or a validated board OTA implementation.** Real-board integration and measurements remain open. There are no signing keys, network services, eFuse operations, production partition changes, or automatic reboot calls in the library.

`Update<B>` is a `no_std`, allocator-independent transaction owner. Platform backends implement `FirmwareBackend`, including their native image format, authentication, durable staging and boot control. The optional simulator reuses `verification::{PublicKey, ContentVerifier, Sha256Verifier}` directly. The optional VFS facade depends on runtime services; neither it nor a runtime/transport/allocator is required by the core.

## Repository audit and integration decisions

- `libraries/file_system/src/operations/base.rs`: existing object-safe polling read/write/control contract, per-opening `Context`, explicit cancellation and single-attempt synchronous adapters. This prototype completes synchronously, returns busy on lock contention and never reports `Pending` without a wakeup.
- `libraries/file_system/src/fundamentals/control.rs`: typed, sized control identifiers; use these rather than introducing another POSIX ioctl implementation. The firmware payloads are fixed arrays of bytes, decoded explicitly; no Rust structs, references, enums or pointers are serialized.
- `subsystems/virtual_file_system/src/file_system/mod.rs`: open permissions are enforced before device open. The new `mount_static_with_permissions` publishes the final 0600 mode under the namespace lock; existing mounts retain their defaults. Mount only as root in trusted composition code, with root-owned non-writable parent directories. The simulator CLI checks root before mounting.
- `subsystems/virtual_file_system/src/synchronous_file.rs`: async, synchronous typed and raw control paths now reject **Write-directed** commands on read-only handles with `InvalidMode`. Queries remain available on write handles, matching existing device conventions. A writable descriptor is a capability: native callers are trusted and must not pass it to an untrusted task.
- `BaseOperations::open` does not receive open flags. Firmware enforces one opening of any mode, without relying on `CreateFlags::Exclusive` (the latter primarily means exclusive creation in current VFS). A second open returns `RessourceBusy`; duplication returns `UnsupportedOperation`. No unbounded handle table is added.
- `executables/wasm/src/host/wasi/file.rs::fd_control`: already bounds control buffers, checks guest ranges/alignment/overlap, and copies into aligned host scratch before raw VFS dispatch. Firmware checks exact input/output lengths, ABI version and reserved bytes before mutation. This PR checks the portable crate on Wasm; a firmware-specific guest execution test remains a follow-up.
- [Platforms hash driver](https://github.com/xila-project/platforms/blob/main/drivers/shared/src/devices/hash.rs): boxed dynamic hash contexts are intentionally not on this streaming path.
- [Platforms Espressif driver](https://github.com/xila-project/platforms/blob/main/drivers/espressif/Cargo.toml): no ESP-IDF dependencies; Platforms’ `.cargo/config.toml` supplies Xtensa linker/runner settings but does not establish functioning OTA linkage, partition layout or secure boot. #324 remains the integration prerequisite.
- #490 supplies verification primitives. XPK (#402/#491), loader enforcement (#495) and filesystem images (#318) remain separate. The simulator manifest below is deliberately a test-backend format, not another application package schema.

## Platform decision record

Source review uses ESP-IDF **v5.5.1**, MCUboot **v2.2.0** and Embassy Boot **0.6.0**. These are reference versions, not added runtime dependencies. No target's linked flash, `.data/.bss`, peak stack, CPU/energy or wear has been measured here.

| Backend / reference board | Trust and update constraints | Decision and unsupported guarantees |
| --- | --- | --- |
| ESP32-S3 / ESP-IDF | Secure Boot v2 uses RSA-PSS-3072 and provisioned eFuse digests. OTA app slots and `otadata`; secure-image padding/signature space, encrypted flash and security version must match the actual build. | Future thin native adapter: `esp_ota_begin` → `esp_ota_write` → `esp_ota_end` → `esp_ota_set_boot_partition`; abort with `esp_ota_abort`, confirm with `esp_ota_mark_app_valid_cancel_rollback`. No second Ed25519 wrapper. `esp_ota_begin` has SDK-owned allocation, so zero heap in this Rust core does not imply zero SDK heap. Standard app OTA does not upgrade the second-stage bootloader. Board evidence and rollback/eFuse ordering required. |
| MCUboot / **nRF52840 DK (nrf52840dk/nrf52840)** | Native signed TLV images; boot trust depends on protected bootloader/provisioning. Swap-offset/move/scratch, overwrite, direct-XIP and RAM-load have different trailers, capacity and recovery behavior. | Select MCUboot swap-offset as the next portable-board evaluation, subject to a concrete Zephyr partition/linker build. Use its own signature and test/confirm/revert APIs. No claim that an unprotected DK has an immutable trust root. Trailer and one-sector offset costs require actual layout evidence. |
| Nordic nRF53/nRF54 | Device-specific immutable stage/NSIB, TrustZone, external flash and network-core coordination. | Separate adapters/partition decisions; multi-image atomicity unsupported by this single-image prototype. |
| STM32 | MCUboot/OEMiROT/SBSFU and protected boot depend on the exact part, option bytes, sectors, banks and RWW restrictions. | No generic STM32 bank-swap assumption. Watchdog/brownout and key provisioning need a chosen board. |
| RP2040 / RP2350 | RP2040 lacks comparable ROM-backed signed secure boot; RP2350 has distinct OTP/security provisioning. XIP erase/program can require RAM execution, IRQ and second-core coordination. | Authenticated updates must not be labeled hardware secure boot on RP2040. Neither board is implemented. |
| Tiny single-bank MCU | May have no safe staging space, recovery root or monotonic counter. | Advertise unsupported safe update/revert; do not overwrite the running image. External storage or a separately reviewed rescue path would be required. |
| Native reference | Ordinary external public-key configuration; mutable raw NOR image. Two slots and redundant checksummed boot records. | `AuthenticatedUpdate` + `FunctionalRevert` only. No hardware-rooted secure boot or hardware anti-rollback; copying an old flash image can restore old metadata. Host file locking prevents cooperating processes from sharing the medium. |
| Wasm | No MCU boot root. | Portable core/simulator compilation only; no board or browser-persistence guarantee. |

MCUboot is preferred for the next portable-board implementation because its native image verification, modes, trailers and reset recovery can be evaluated together. Embassy Boot is smaller in scope and worth measuring, but its exact version/configuration matters: 0.6.0's `verify_and_mark_updated` is feature gated and verifies an ordinary Ed25519 signature over a **SHA-512 digest**, with a two-byte hashing scratch buffer in the reviewed code; unverified `mark_updated` exists when verification is disabled. That is not the simulator's canonical manifest signature and does not establish hardware-protected verified boot. Neither project's generic source can supply meaningful board-linked size/stack numbers.

Reviewed upstream sources:

- [ESP-IDF v5.5.1 OTA API and lifetime/error contracts](https://github.com/espressif/esp-idf/blob/v5.5.1/components/app_update/include/esp_ota_ops.h)
- [ESP32-S3 Secure Boot v2](https://docs.espressif.com/projects/esp-idf/en/v5.5.1/esp32s3/security/secure-boot-v2.html)
- [MCUboot v2.2.0 image/trailer/swap/recovery design](https://github.com/mcu-tools/mcuboot/blob/v2.2.0/docs/design.md)
- [Embassy Boot 0.6.0 updater signature path](https://github.com/embassy-rs/embassy/blob/embassy-boot-v0.6.0/embassy-boot/src/firmware_updater/asynch.rs)

## Transaction and trust contract

Open → `BEGIN` → partial `write` loop → `FINISH` → explicit `ACTIVATE` → close. `FINISH` authenticates staged flash readback and creates a private backend-owned proof. The core holds that proof exclusively. The simulator binds it to metadata generation, slot and complete signed manifest, then reauthenticates flash at activation. Length overflow, truncation, authentication failure and storage errors poison the transaction; activation is unavailable afterward. `ABORT` clears staging, never pending boot. Closing/dropping a VFS handle aborts staging and never activates.

Persistent boot intent is reconstructed from flash, independently of the core's RAM stream state. Boot selection validates the signed manifest and complete image. A pending image gets one trial: persist trial intent **before** selecting it. The next boot without confirmation reverts to confirmed. Confirmation uses a fresh root-only writable VFS handle after the same simulated runtime has booted and passed its chosen health checks. Repeated confirm is idempotent. This harness's `boot --confirm` models successful health checks; it does not test Xila scheduler/filesystem/network health or execute the image as machine code.

The next confirmed security floor is committed with the new confirmed image, so a failed confirmation leaves the old rescue image usable. This is a software policy in mutable storage, not a hardware monotonic counter. Real adapters must reconcile irreversible counters, rescue compatibility and trial confirmation themselves. Firmware signing does not authenticate provisioned LittleFS/Wasm applications: their trust/compatibility policy is separate.

The public API never accepts a flash address, caller-chosen slot, trust key or signing key. Trusted native composition supplies the backend's public key. `init` is a manufacturing/test command, refuses existing/non-erased media, and requires an externally signed factory fixture. A torn factory provision requires explicit rescue; it is not automatically reformatted.

## Byte ABI v1

Control kind is `F`. Prefix: bytes 0–1 contain `1` as little-endian u16; bytes 2–7 are zero. Exact payload sizes are required, including zero-length output for mutations.

| Command number | Direction | Input | Output |
| --- | --- | --- | --- |
| 0 `GET_CAPABILITIES` | Read | 8-byte prefix | 32 bytes: prefix, max image u64 at 8, max chunk u32 at 16, guarantee bits u32 at 20; rest zero |
| 1 `GET_STATUS` | Read | prefix | 24 bytes: prefix, stream state u8 at 8, zero 9–15, received u64 at 16 |
| 2 `BEGIN` | Write | 280 bytes: prefix, image size u64 at 8, metadata length u16 at 16, zero 18–23, up to 256 metadata bytes at 24, zero trailing capacity | none |
| 3 `FINISH` | Write | prefix | none |
| 4 `ACTIVATE` | Write | prefix | none |
| 5 `ABORT` | Write | prefix | none |
| 6 `CONFIRM` | Write | prefix | none |

All integers are explicitly little-endian; alignment of these byte arrays is one. Stream states: idle 0, receiving 1, verified 2, pending boot 3, failed 4. Status describes this session's stream, not a bootloader's persistent state. Persistent native diagnostics are available through `inspect`. Guarantee bits: authenticated update 1, functional revert 2, verified boot 4, hardware root 8. The simulator reports **3**, never 4 or 8.

Errors map to existing Xila codes: authentication/target/security version → `PermissionDenied`; storage/recovery → `InputOutput`; unsupported → `UnsupportedOperation`; state/length/encoding/geometry → `InvalidParameter`. VFS read-only mutation → `InvalidMode`; second open → `RessourceBusy`. Tests assert these typed codes; their existing file-system ABI discriminants are reused.

## Reference flash and image formats

NOR geometry is intentionally small for exhaustive tests: 256-byte erase sectors, 64-byte program pages, minimum write one byte, erased value FF, program only 1→0, no program crossing a page. This is simulation geometry, not an ESP partition table. Durable write counts include only successfully completed page programs; on any program error the transaction is invalid even if a prefix reached flash.

| Region | Offset | Bytes |
| --- | --- | --- |
| Boot record 0 / 1 | 0 / 256 | 256 each, first 64 used |
| Slot A / B | 512 / 4608 | 4096 each |
| Per-slot signed metadata | slot start | 144 in reserved 256-byte header sector |
| Per-slot image | slot start + 256 | at most 3840 |

Total flash backing: **8704 bytes**. Each 64-byte journal record has magic `FWJ1`, generation u64, confirmed/pending slot bytes, trial byte, software security floor u64, reserved zeros and SHA-256 over its first 32 bytes. `BEGIN` first commits a fresh generation, invalidating proofs retained across abort or backend reconstruction. Write the other record's sector while keeping the newest valid record untouched. Select the largest valid generation; ties/no valid record require recovery. The checksum detects torn/corrupt metadata; it does not authenticate metadata against a malicious host.

The simulator-only manifest is 144 bytes: exact 16-byte domain `XILA-FW-SIM-v1\0\0`, target bytes 16–31, logical size LE u64 at 32, security version LE u64 at 40, SHA-256 at 48–79, strict ordinary Ed25519 signature over **exactly bytes 0–79** at 80–143. No public key comes from this manifest. This format is not accepted by ESP-IDF or MCUboot.

`MemoryNor` borrows caller storage; `FileNor` streams raw file I/O with fixed scratch and holds an exclusive OS file lock. It writes/syncs an injected prefix before returning failure; no whole-image buffer, rename or host journal is used to stand in for NOR atomicity. Host `sync_data` is a persistence aid for process tests, not proof of physical flash/cache/brownout behavior. Repeated zero-progress I/O is not treated as success. Faults select zero-based erase/program operation and a byte prefix (0 = before; operation length or larger = after). The medium rejects further mutation after the cut; reconstruct it to model reboot.

## Reproduce

```sh
mise run firmware:test
mise run firmware:integration
cargo clippy --locked -p xila-firmware-update --features native --all-targets -- -D warnings
cargo check --locked -p xila-firmware-update --features simulator --lib --target wasm32-unknown-unknown
```

The [Platforms firmware simulator](https://github.com/xila-project/platforms) and its process tests create public-key/signed synthetic fixtures using a deterministic **test-only** seed and start separate `xila-firmware-sim` processes for staging, trial, revert and confirmation. No private production key is present. From a Platforms checkout, supply your own **test** public key, manifest and image files:

```sh
cargo run --bin xila-firmware-sim -- \
  init --flash /tmp/fw.img --key test-public-key.bin --target xila-native-test \
  --manifest factory.manifest --image factory.bin
# Same public key/target options are required for every command below.
xila-firmware-sim update --flash /tmp/fw.img --key test-public-key.bin --target xila-native-test \
  --manifest next.manifest --image next.bin
xila-firmware-sim boot --flash /tmp/fw.img --key test-public-key.bin --target xila-native-test --confirm
xila-firmware-sim inspect --flash /tmp/fw.img --key test-public-key.bin --target xila-native-test
```

`cargo run` does not install the binary in PATH; use `target/debug/xila-firmware-sim` or the same `cargo run ... --` prefix. `update --stage-only` closes without activation. `update` or `boot --fault-operation N --fault-byte K` injects a torn operation; rerun without fault flags to simulate reset.

Tests cover positive signed streams; wrong key/target/size/version; malformed metadata; corruption at finish, activation and boot; erase/program geometry and bit rules; every update operation at before/after/selected byte offsets; boot/confirm journal cuts; seeded schedules (`0x526f1a5`, seed and case on failure); root permissions, read-only modes, exclusivity, duplicate rejection and close behavior; file locking and child-process persistence. Core CI runs the simulator and device suites and no_std/Wasm compilation; Platforms CI runs the child-process suite. This is deterministic adversarial testing, not a claim of exhaustive physical hardware validation or a fuzzing campaign.

## Measurements and remaining gates

The allocation regression instruments the Rust allocator with a positive control and measures provision, begin/write, strict signature/readback, activate, boot and confirm: **zero heap allocation requests** in the core and in-memory backend path. The VFS/runtime/harness allocates separately (one bounded context marker per open, namespace/runtime state and a 64 KiB test filesystem); the native file backend uses OS resources. Flash storage is not RAM. On the native x86_64 build the simulator is 368 bytes and its transaction owner is 608 bytes; these figures include bounded state, not the backing medium. The test prints native `size_of` values for the backend/transaction and backing flash; these are not peak stack, linked flash or target ABI results.

Before any board implementation is accepted: produce actual signed boot/provisioning and recovery builds, a partition map, before/after linked flash and `.data/.bss`, peak signature/OTA stack, SDK/driver heap, caller/alignment buffers, reset and fault evidence, watchdog/brownout and flash-encryption behavior, monotonic-counter ordering and filesystem compatibility. Validate one ESP32-S3 and the chosen MCUboot board; assess single-bank, XIP, multicore and read-only capabilities explicitly. Do not burn eFuses or change irreversible protection on ordinary development boards automatically. Keep #526 open until those acceptance criteria have evidence.
