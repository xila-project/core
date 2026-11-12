# Native application ABI

`xila-abi-application` is the stable C interface for native applications linked
against Xila. It is `no_std` with `alloc`; Wasm guests keep their separate WASI
implementation. The facade does not enable this crate by default.

The interface prioritizes RAM, flash and CPU footprint. Files wrap an owned
synchronous VFS file, and directories retain only a VFS reference, task identity,
canonical absolute path and one byte of rights. There is no global handle table,
lookup or context argument. Distinct opaque `XilaFile` and `XilaDirectory` pointer
types catch accidental type confusion in C. Read and write use caller buffers
directly, without temporary allocations or hot-path logging. Opening allocates
only the joined path and returned object, in addition to underlying VFS costs.

Directory capabilities bound filesystem authority: paths are UTF-8 pointer/length
pairs, relative to the supplied capability. Absolute paths, parent components,
embedded NUL and invalid UTF-8 are rejected; dot and empty components are
canonicalized. Where an entry is required (file open, create, remove, rename), a path naming the capability itself (`""`, `"."`) is invalid. Child rights can only decrease.
Read needs the READ right; write, create, truncate and append need the WRITE
right, and truncate or append are only valid with `XILA_OPEN_WRITE`. Native code remains trusted: pointers must be live, calls on each object serialized,
and a path capability does not pin an inode against concurrent namespace changes.
VFS symbolic links are followed, including absolute targets: the lexical checks
are not a symlink sandbox. Strict subtree confinement needs a future VFS
root-constrained resolver; loaders must control the namespace they hand out.

Errors are explicit `XilaStatus` values with no errno or implicit task state.
Required null pointers fail with InvalidArgument; zero-sized I/O permits a null
buffer, seek's position output is optional, and null close succeeds. Other
pointers must meet the documented validity, alignment, aliasing and ownership
requirements. Outputs change only on success. Close always consumes ownership,
including when the VFS reports a close error. Allocation exhaustion follows the
runtime allocator's failure policy; recoverable VFS allocation errors map to
OutOfMemory.

## Functions

- `xila_abi_version`: packed major/minor version, currently 0.1.
- `xila_directory_open`, `xila_directory_close`: attenuated path capabilities.
- `xila_file_open`, `xila_file_close`: typed owned file objects.
- `xila_file_read`, `xila_file_write`: direct buffer I/O.
- `xila_file_seek`, `xila_file_flush`: cursor and persistence operations.
- `xila_file_get_statistics`, `xila_directory_get_statistics`: readable metadata.
- `xila_directory_iterator_open`, `xila_directory_iterator_next`,
  `xila_directory_iterator_rewind`, `xila_directory_iterator_close`: owned listings.
- `xila_directory_create`, `xila_directory_remove`, `xila_directory_rename`: namespace changes.
- `xila_file_set_size`: resize writable files without changing their cursor.
- `xila_time_monotonic`, `xila_time_realtime`: checked nanosecond clock reads.

The host constructs capabilities with `Directory::new` and wraps existing streams
with `File::new`. `into_raw` transfers ownership and unsafe `from_raw` reclaims it
exactly once. The host installs its time manager with `initialize`; clock reads
before initialization and unsynchronized realtime reads return Unavailable.

## Header and compatibility

The build generates `application.h` into `OUT_DIR` using cbindgen, without macro
expansion or nightly. The committed public header is `include/xila/application.h`.
Run `XILA_UPDATE_HEADER=1 cargo test -p xila-abi-application generated_header` to
refresh it; normal tests require an exact match. The separate unpublished
`xila-abi-application-c-tests` member compiles and executes a strict C11 client,
so ordinary consumers do not need a C compiler for this crate's header generation.

Until the first release the ABI stays at 0.1. After it, additive changes bump the minor version; breaking changes bump the major
version. Never reuse or renumber constants or status values. Rust object layouts
are private and must never be inspected or allocated by C callers.

## Future scope

Tasks/threads, networking and graphics are not included. Dynamic loading belongs to the future native loader ([Core issue #334](https://github.com/xila-project/core/issues/334)).
A POSIX compatibility layer can be built above this native interface later.

## Filesystem metadata and listings

Metadata and listing require READ; namespace changes and resize require WRITE.
File metadata works on any open file, like POSIX `fstat`; resize requires a writable open.
`XILA_STATISTICS_NO_FOLLOW` inspects a final symlink; empty metadata/listing paths
refer to the supplied directory. Symlink creation is deliberately not exposed.
`XILA_REMOVE_DIRECTORY` permits directories in addition to files; without it,
directory removal returns IsADirectory. Removal does not follow the final link.
Rename requires the same VFS and task for both capabilities and follows backend
replacement semantics. Namespace capabilities remain subject to the concurrent
namespace and symlink limitations described above.

`XilaStatistics` is a 72-byte `repr(C)` struct. Its fields in order are six
`uint32_t` values (`size`, `kind`, `permissions`, `owner_user`, `owner_group`,
`reserved`), three `uint64_t` values (`file_size`, `link_count`, `inode`), and
three `int64_t` Unix nanosecond times (`access_nanoseconds`,
`modification_nanoseconds`, `status_change_nanoseconds`). Permissions are Unix
mode bits; reserved is zero. Kind constants cover file, directory, block device,
character device, pipe, socket and symlink. VFS timestamps currently have second
precision; conversion returns Overflow when signed nanoseconds cannot represent them.

Before every metadata call, the caller sets `size = sizeof(XilaStatistics)`.
The first published version requires at least 72 bytes; smaller sizes are rejected
without writing outputs. The callee writes only `min(size, its own size)` bytes;
larger callers' trailing bytes are untouched. The returned size identifies the
callee's layout. Future fields will preserve the 72-byte prefix and minimum size.

Listing copies UTF-8 names without a trailing NUL and skips `.` and `..`.
End (14) leaves outputs unchanged. BufferTooSmall (15) is the sole exception to
success-only outputs: it writes the required `name_length`, leaves kind and buffer
unchanged, and retains the entry for retry. A null buffer is allowed at zero
capacity. Rewind resets the listing and any pending entry only on success.
DirectoryNotEmpty (16) reports removal of a nonempty directory. Closing always
consumes an iterator, even on error; closing null succeeds. The VFS supplies owned
String names, so one allocation per entry is unavoidable; retries add none.

## Linked application startup

The C entry convention is `int32_t xila_main(const XilaStartup *startup)`.
Firmware links applications statically today. Each application therefore exports
its own unique entry name (for example `c_demo_main`), and the Rust host passes
that function pointer to `run_main`; there is no required global `xila_main` symbol.
Wasm remains the installable application format; this convention adds no loader.

`XilaStartup.size` is the structure size in bytes; check it before reading fields.
Arguments are UTF-8 `XilaString` pointer/length pairs, without NUL terminators.
Any pointer may be null when not granted; a null argument array has count zero.
The host owns the startup structure, strings and initial file/directory objects.
They are borrowed only until the entry returns: do not retain or close them.
Closing a startup object is undefined behavior. Close objects opened by the app.
The Rust-only unsafe `run_main` helper consumes optional capabilities, lends them
to the entry, and drops them after return (closing streams). It allocates only the
argument descriptor array; no capability boxes or global handle table are needed.
Directory listing, statistics, rename/remove, tasks/threads and graphics
are not included. Program entry and handing initial capabilities to an application
belong to the future native loader ([Core issue #334](https://github.com/xila-project/core/issues/334)).
A POSIX compatibility layer can be built above this native interface later.

## Optional networking (`network`)

Networking is disabled by default. The host constructs `Network::new(manager,
rights)` and grants its opaque pointer; no call obtains ambient authority.
CONNECT authorizes TCP connect and UDP send, LISTEN authorizes TCP listen/accept
and UDP bind, RESOLVE authorizes DNS. Accepted and connected TCP objects grant
bidirectional I/O; UDP retains the original rights. Closing a capability does not
revoke its sockets. Host/port restrictions can later be added to the capability's
Rust representation and checked at connect, bind, send and resolve.

The ABI uses Core's smoltcp network manager, not OS sockets. The host must mount
and configure an interface and keep its runner on an independent executor/thread:
synchronous calls use `task::block_on`, like synchronous VFS calls (the current
adapter spins while pending). Do not call blocking operations on the runner's
thread. TCP connect waits indefinitely when timeout_ms is zero; a nonzero value
bounds the handshake in milliseconds. TCP read/write and accept block, TCP sends
may be partial, receive zero means EOF, and zero-length I/O succeeds immediately.
Only write shutdown is supported (FIN); close aborts and may discard queued data.
A listener holds one socket, then replenishes it on accept; there is no backlog.
If replenishment fails the accepted connection remains in the listener for retry.
Core does not currently detect duplicate endpoint binds like an OS would.

UDP I/O probes once and returns WouldBlock if pending. It accesses the underlying
socket through Core's accessors to avoid the logging in Core's UDP convenience
methods. Datagrams are atomic; overflow preserves the received datagram and caller
buffer. Each socket uses 1024-byte transmit and receive buffers; UDP additionally
uses one metadata slot per direction. These buffers are allocated by Core at
creation. Send/receive add no allocation, logging or staging buffer. IPv4 occupies
the first four network-order bytes, the other bytes must be zero; IPv6 uses all
16. Ports and other integer fields are host order. Reserved fields and scope_id
must be zero; scoped IPv6 is unsupported. Core rejects port zero for binding.

DNS selects IPv4 or IPv6 explicitly and returns port zero. Core allocates a vector
for DNS results. Count is capacity on input, number returned on success. Capacity
zero queries the required size; an undersized nonempty array returns Overflow
without changing outputs. A second query may yield a different count. Network
errors have dedicated status numbers 26–35; existing values are unchanged.

The generated header always declares networking, independent of Cargo features;
linking those functions requires `network`. cbindgen parses the unexpanded source,
so the committed header is identical with and without the feature. The C test
member explicitly enables it and exercises smoltcp loopback TCP echo, UDP round
trip, capability failures and numeric DNS.
