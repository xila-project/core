# io_error

The shared failure vocabulary of file systems, the VFS, device files and network transport.

- `Kind`: one fieldless, `#[repr(u16)]` kind (2 bytes) with a stable, explicitly assigned ID. File system, VFS and network operations all return it, so a failure keeps its meaning from the producer to the caller. `file_system::IoErrorKind`, `virtual_file_system::IoErrorKind` and `network::IoErrorKind` re-export it.
- `ResolveError`: the compact typed DNS result. It separates a malformed name, an unavailable network and a confirmed negative answer from an unknown failure. Each variant has the ID of the `Kind` it projects to.
- Boundary translations, each done once: `embedded_io::Error` (`Kind` implements it), `Kind::posix_errno()` (a pure projection to a symbolic errno for the WASI mapper and the future libc shim), the native ABI `Status` (generated from the catalog) and `ResolveError::getaddrinfo()`.
- Localized `Display` in the nine shipped locales. `Debug` prints the ID only, so firmware that formats an error does not retain a table of variant names.

## IDs

IDs are stable. Zero is success and is never a kind. A band describes the meaning of a kind, not its producer:

| Band | Meaning |
|---|---|
| `0x0100..=0x01FF` | common |
| `0x0200..=0x02FF` | storage and paths |
| `0x0300..=0x03FF` | devices and privilege |
| `0x0400..=0x04FF` | transport |
| `0x0500..=0x05FF` | resolver |
| `0x0600..=0x06FF` | reserved for future kinds |
| `0x0700..=0x07FF` | native-ABI-only outcomes (`End`, `BufferTooSmall`), never a `Kind` |

Never reuse a retired ID. Decode integers with `Kind::try_from(u16)`; an unknown ID is reported as `UnknownKind`, never reinterpreted. `Kind` is `#[non_exhaustive]`: exhaustive matches belong to the owners of a mapping (`catalog.rs`, `posix.rs`, `display.rs`, the `Status` generator).

## Adding a kind

1. Add a row to `catalog.rs` with a new ID in the right band: errno and embedded-io projection included.
2. Add its message key to `display.rs` and to all nine `locales/*.json` files (`mise run localization:lint` checks the French typography rules).
3. Add its row to `tests/catalog.rs`. The ABI `Status` enum, its C declarations and the frozen Status↔Kind table (`libraries/abi/application`) follow from the catalog; regenerate the header with `XILA_UPDATE_HEADER=1 cargo test -p xila-abi-application`.

## Choosing a kind

Classify at the producer closest to the evidence and keep that kind through the VFS.

- `WouldBlock`: the operation cannot make progress now and the caller can wait for readiness (a pending synchronous probe, a contended try-lock, an empty non-blocking pipe). `ResourceBusy`: the resource is in use administratively and waiting for readiness will not help (unmounting a file system with open files, a second open of an exclusive device).
- `NoSpace` is storage exhaustion; `OutOfMemory` is memory exhaustion.
- `SymlinkLoop` is the link limit; `CorruptData` is a link whose stored target is unusable.
- `BadDescriptor` is a descriptor or access mode that does not permit the operation; `PermissionDenied` is an authorization decision.
- `InputOutput` is a generic hard failure. It never implies a retry.
- Task, users and time errors enter the I/O path through `file_system::IntoIoErrorKind`, an explicit mapping at the call site.
