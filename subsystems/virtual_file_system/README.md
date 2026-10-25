# Virtual file system

## Trust and task context

Native code is trusted kernel code. VFS permission checks provide a consistent
API and prevent accidental misuse of task context; they do not isolate native
components from each other. Untrusted code belongs in the WASM runtime, whose
host bindings must supply the guest's task context and enforce its capabilities.
Calling a backend directly from native code is a trusted kernel operation.

Files opened through the VFS retain the opening task's authority. Duplicates
retain the same authority, while subsequent user changes on that task are read
at each metadata mutation. Authority is bound to the manager and a non-reused
registration generation: task exit or numeric ID reuse invalidates it. Moving a
handle between native components retains that opening context; native callers
that need another task's context should open a handle for that task.

## Metadata mutation

Path and descriptor mutations share the owner/root authorization policy.
Changing path ownership is root-only. Descriptor ownership changes currently
return `UnsupportedOperation` for root and are denied for non-root callers;
use the path API for supported ownership changes. Read/write access alone never
grants permission to change a foreign file's metadata.

Synchronous descriptor mutations attempt the existing namespace and task locks
once, returning `RessourceBusy` on contention. They do not block an executor
waiting for those locks. Path metadata mutations use the namespace write guard
to serialize authorization and mutation with rename and descriptor mutations.
Lock order is namespace, then a short task credential lookup, then backend
filesystem and file context. Backend operations remain synchronous probes;
no polling I/O contract is changed.

LittleFS synchronizes configured metadata buffers across independent opens
under its filesystem lock. It matches native metadata-pair/entry identities,
not saved pathname prefixes; unlink/replacement must not update old objects.
A namespace metadata commit uses a short-lived native open to pin that identity
through metadata relocation. It adds a temporary native cache of the configured
LittleFS cache size, released before returning.

LittleFS descriptor metadata mutations have explicit limitations:

- Read-only opens return `UnsupportedOperation` rather than reporting a change
  that disappears on close.
- A native `DUSTY` open has stale file data after another independent open's
  commit. Its metadata mutation returns `RessourceBusy`; reopen the file.
- An `ERRED` open returns `InputOutput`. Metadata cache changes are published to
  other opens only after a successful commit.
- Renaming an open source file returns `RessourceBusy`; close it and retry.
  Native LittleFS otherwise detaches those source handles during rename.

Independent writer/append consistency and failing close/Drop semantics remain
separate work; these restrictions do not implement a complete POSIX interface.
WASI traversal confinement is also separate from metadata authorization.

## Allocation observations

Same 256-byte-cache fixture on an x86_64 host, counting Rust `GlobalAlloc`
requests on the executing test thread:

| Workload | Before | After |
| --- | ---: | ---: |
| Synchronous file object size | 80 bytes | 104 bytes |
| Open an existing file | 11 requests | 11 requests |
| Descriptor chmod | 0 requests | 0 requests |
| Path chown with an open file | 4 requests | 4 requests |
| One read / one write | 0 / 0 requests | 0 / 0 requests |

These request counts exclude LittleFS's native allocator; the temporary probe
cache described above is additional. Task metadata adds one `u64` generation,
and each manager adds one `u64` counter, without registration heap allocations.
These observations are not MCU measurements, a peak-heap benchmark, or a latency
or binary-size comparison.
