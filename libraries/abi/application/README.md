# Native application ABI

`xila-abi-application` is the stable C interface for native applications running
on Xila. It is `no_std` with `alloc`; Wasm guests keep their separate WASI
implementation. The facade does not enable this crate by default.

The interface prioritizes RAM, flash and CPU footprint. Files wrap an owned
VFS file handle, and directories retain only a VFS reference, task identity,
canonical absolute path and one byte of rights. There is no global handle table,
lookup or ambient context. Distinct opaque `XilaFile` and `XilaDirectory` pointer
types catch accidental type confusion in C. Read and write use caller buffers
directly, without temporary allocations or hot-path logging. Opening allocates
only the joined path and returned object, in addition to underlying VFS costs.

## Cooperative async bridging

On Linux x86_64/AArch64 and supported single-core bare-metal targets,
native ABI calls await VFS and network futures through
`xila-stackful::suspend_on`. A pending operation suspends the coroutine and
lets other tasks run on the same executor. A device or subsystem wakes the
coroutine when the operation can progress.

Outside a coroutine, including on targets without stack switching, async
bridges retain the `task::block_on` fallback. File read, write and flush
keep their single-poll behavior outside a coroutine. Nonblocking file I/O
keeps that behavior inside coroutines as well. A pending file operation or
an unavailable UDP datagram returns `WouldBlock` on these probe paths.

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

- `XilaApi.version`: packed major/minor version, currently 0.1.
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
- `xila_heap_allocate`, `xila_heap_reallocate`, `xila_heap_free`: process-owned allocation.

The host constructs capabilities with `Directory::new` and wraps existing streams
with `File::new`. `into_raw` transfers ownership and unsafe `from_raw` reclaims it
exactly once. The host installs its time manager with `initialize`; clock reads
before initialization and unsynchronized realtime reads return Unavailable.

## Header and compatibility

The `#[repr(C)] XilaApi` struct in `src/api.rs` defines the slot order and
signatures. All calls return the 32-bit `XilaStatus`. The table starts with
the packed version and total byte size, then one function pointer per slot.
Builds without the `network` feature keep the network slots and return
`XILA_STATUS_UNSUPPORTED` from them.

cbindgen generates `include/xila/application.h` without macro expansion. The
build also parses `XilaApi` with syn to produce the SDK stubs and the zero-based
slot snapshot `sdk/slots.tsv`. The build checks every slot offset and the table
size on the compilation target. Refresh and verify the committed files with:

```sh
XILA_UPDATE_HEADER=1 cargo test -p xila-abi-application generated_header
cargo test -p xila-abi-application-c-tests
```

ABI 0.1 is pre-release. Rebuild native apps against the matching SDK when the
table or status numbers change. After the first release, append slots for minor
versions and increment the major version for incompatible changes. Keep Rust
object layouts opaque to C callers.

Status zero is success. The unified status ranges are `0x0100–0x01FF` for
common errors, `0x0200–0x02FF` for storage and paths, `0x0300–0x03FF` for
device and task errors, `0x0400–0x04FF` for transport, and `0x0500–0x05FF`
for DNS. The `0x0600–0x06FF` band is reserved and `0x0700–0x07FF` contains
native-only outcomes. ExecFormat, NoChild, ArgumentListTooLong, Deadlock and
InappropriateIoControl hold the IDs for ENOEXEC, ECHILD, E2BIG, EDEADLK and
ENOTTY. Existing Rust subsystems are still translated into the status space at
the ABI boundary.

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
End (0x0701) leaves outputs unchanged. BufferTooSmall (0x0702) is the sole exception to
success-only outputs: it writes the required `name_length`, leaves kind and buffer
unchanged, and retains the entry for retry. A null buffer is allowed at zero
capacity. Rewind resets the listing and any pending entry only on success.
DirectoryNotEmpty (0x0209) reports removal of a nonempty directory. Closing always
consumes an iterator, even on error; closing null succeeds. The VFS supplies owned
String names, so one allocation per entry is unavoidable; retries add none.

## Linked application startup

A linked C entry has the signature `int32_t c_demo_main(const XilaStartup *startup)`.
Rust passes the entry pointer to `run_main`. Built-in applications such as
`c_demo` include `application.h` and call through the instance's `startup->api`.
For example, `startup->api->file_write(startup->standard_output, data, length, &written)`
writes to the borrowed output stream. Read the ABI version from
`startup->api->version`.

`XilaStartup.size` is the structure size in bytes. Check it before reading its
fields, and check `api->version` and `api->size` against the required slots.
The API pointer is always present. Optional capability pointers may be null.
Arguments are UTF-8 `XilaString` pairs without NUL terminators. The argument
array is null at count zero.

The host lends the startup data, strings and initial capabilities for the call.
The app may close its own file and directory objects. It must leave borrowed
objects open and release access when the entry returns. The Rust helper drops
the startup resources when the entry returns. The caller supplies the process
heap and owns its teardown.

## SDK startup

A separately linked C image defines `int32_t xila_main(const XilaStartup *)` and
links `sdk/crt0.c` and `sdk/stubs.c`, using `-Iinclude/xila -Isdk` and including
`stubs.h`. The host calls `_xila_start` once per image. crt0 checks the startup
and table sizes and the ABI version, installs the image-local table pointer,
then calls `xila_main`. Before startup and after `xila_main` returns, all stubs
return `XILA_STATUS_UNAVAILABLE`. The entry must join any users of the stubs
before returning. Invalid startup returns -1.
Built-in callers use their startup pointer directly so concurrent executions can
borrow different capabilities.

## Process heap

Each startup can grant a `XilaHeap`. Heap calls take the capability explicitly
and callers serialize operations on it. Heap storage is uninitialized, including
new bytes after a resize. Allocation of zero bytes reserves one byte.
Reallocation with a null pointer allocates, and a new size of zero frees and
stores null. Free accepts null. An unrepresentable size or allocator failure
returns OutOfMemory and preserves the original block and output. Free and
resize return `XILA_STATUS_INVALID_ARGUMENT` for a pointer that this heap does
not own, an interior pointer or a block that was already freed.

Each block starts with a 16-byte header on 32-bit targets (Arm and RV32) and
32 bytes on 64-bit targets: previous and next links, the size and an owner tag.
The doubly linked list gives constant-time free and resize. The tag is a
per-heap key XOR the block address, and free clears it. Free and resize check
the alignment and the tag before they unlink anything. The heap keeps no side
table: its metadata never allocates, never fails and never outlives the block,
so a shrinking resize cannot fail because of it.

Native applications are trusted, so this is cheap misuse detection rather than
hardening. Like a C allocator, the check reads the header below the pointer, so
the pointer must address readable memory. A freed block whose memory a later
allocation of the same heap reuses identifies that new allocation, as with
malloc.

The process owner drops the heap after all C execution ends. A loader must
keep that owner outside the native coroutine stack, including when it cancels
an unfinished coroutine.

## Optional networking (`network`)

Networking is disabled by default. The host constructs `Network::new(manager,
rights)` and grants its opaque pointer; no call obtains ambient authority.
CONNECT authorizes TCP connect and UDP send, LISTEN authorizes TCP listen/accept
and UDP bind, RESOLVE authorizes DNS. Accepted and connected TCP objects grant
bidirectional I/O; UDP retains the original rights. Closing a capability does not
revoke its sockets. Host/port restrictions can later be added to the capability's
Rust representation and checked at connect, bind, send and resolve.

The ABI uses Core's smoltcp network manager, not OS sockets. The host must mount
and configure an interface. When called from inside a `xila-stackful` coroutine
on a supported target, blocking TCP and DNS calls suspend via
`stackful::suspend_on`, so native callers can share the same single-threaded
executor with the smoltcp interface runner without starving it. Outside a
coroutine (or on targets using the `task::block_on` fallback), keep the
interface runner on an independent executor/thread and do not call blocking
operations on the runner's thread. TCP connect waits indefinitely when
timeout_ms is zero; a nonzero value bounds the handshake in milliseconds. TCP
read/write and accept block, TCP sends may be partial, receive zero means EOF,
and zero-length I/O succeeds immediately.
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
without changing outputs. A second query may yield a different count. The C test
member enables networking and exercises smoltcp loopback TCP echo, UDP round
trips, capability failures and numeric DNS.
