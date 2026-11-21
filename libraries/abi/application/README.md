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
- `xila_time_resolution`, `xila_time_sleep`: clock quantization and cooperative sleep.
- `xila_random_fill`: bytes from the host-granted entropy source.
- `xila_heap_allocate`, `xila_heap_reallocate`, `xila_heap_free`: process-owned allocation.
- `xila_process_id`, `xila_process_parent_id`, `xila_process_exit`: identity and termination.
- `xila_process_spawn`, `xila_process_wait`, `xila_process_child_close`: children.
- `xila_env_get`, `xila_env_set`, `xila_env_remove`, `xila_env_entry`: the process's environment.
- `xila_graphics_call`: one graphics call (see [Optional graphics](#optional-graphics-graphics)).

The host constructs capabilities with `Directory::new` and wraps existing streams
with `File::new`. `into_raw` transfers ownership and unsafe `from_raw` reclaims it
exactly once.

## Readiness and descriptor operations

`xila_poll_wait` takes an array of `XilaPollEntry` records. Set each kind and
handle, and request `XILA_POLL_IN` and/or `XILA_POLL_OUT`. The call returns the
number of entries with nonzero `revents`. Errors and hangups are reported even
when neither data event was requested. A null handle or invalid kind produces
`XILA_POLL_NVAL`. Use `XILA_POLL_IGNORE` to skip an entry. As with the other
native ABI calls, an arbitrary non-null pointer is outside the contract.

Zero timeout probes once. Positive timeouts are relative milliseconds on the
executor clock, and negative timeouts wait indefinitely. A wait that would
block outside a coroutine returns `WouldBlock`. An empty set may be used to
sleep in a coroutine. Regular files and block devices report readiness for
operations allowed by the opening, including at EOF. A write-only regular file
reports `XILA_POLL_OUT` when requested. A request for `XILA_POLL_IN` alone on
that opening reports no events. Directory capabilities report read readiness.
Character devices without a probe always report the requested directions
allowed by the opening. Poll never waits for readiness on these devices. If
all reported events come from this fallback, the coroutine yields once before
returning, even for a zero-time probe. A blocking read can still suspend on
the device after polling a set that includes both that device and a pipe.
Use nonblocking reads to handle `WouldBlock`. Drivers should implement precise
readiness with a non-consuming snapshot and notify the shared hub when
readiness changes. Applications Terminal and the Platforms std console are
follow-ups. With the `network` feature, TCP
connections, TCP listeners and UDP sockets are pollable. The network runner
notifies the hub when socket state changes, without taking over the individual
socket's read or write waker. Keep sockets live and serialize their other ABI
calls while polling. Multiple pollers may wait on the same socket or file.

Pipe readiness tracks queued bytes, free space and peer shutdown. The last
writer closing reports `XILA_POLL_HUP` to readers, together with
`XILA_POLL_IN` when bytes remain. Once drained, reads return zero. The last
reader closing reports `XILA_POLL_ERR` to writers, and a nonempty write then
returns `BrokenPipe`. A file readiness snapshot keeps a pipe's storage
accessible without keeping its endpoints open. The wait registers one hub
waker and stores one result per entry. The hub and snapshots are released on
completion, timeout or coroutine cancellation. The hub currently wakes all
pollers on a readiness notification, and each poller rescans its sources.
Registering waiters per source is a follow-up.

`xila_pipe_create` uses the supplied process capability to return an owned read
and write handle for a 512-byte unnamed pipe. Pass `XILA_OPEN_NON_BLOCKING` for
probe-only I/O. Writes up to the SDK's 512-byte `PIPE_BUF` are atomic. If
the whole write does not fit, a blocking writer waits and a nonblocking writer
returns `WouldBlock` without queuing bytes. Writes larger than `PIPE_BUF` can
transfer partially even in blocking mode. Blocked writers have no FIFO
fairness guarantee. `XILA_POLL_OUT` on a pipe requires room for an atomic write.

`xila_file_control` accepts an aligned native-endian `uint32_t` buffer.
`XILA_CONTROL_BYTES_AVAILABLE` and `XILA_CONTROL_GET_FILE_FLAGS` take zero
input bytes and return the pipe's queued byte count or the file's access mode
and status flags. Either pipe endpoint can query the byte count.
`XILA_CONTROL_SET_FILE_FLAGS` takes the new `XILA_OPEN_APPEND` and
`XILA_OPEN_NON_BLOCKING` bits and zero output bytes.
`xila_file_is_terminal` queries the VFS terminal capability and can suspend
a coroutine if the device needs to wait.

`xila_file_duplicate` returns an independently closable handle that shares
the original VFS cursor and status flags. Duplicating an SDK directory
descriptor creates another path capability. Directory streams opened from the
capability have independent `readdir` positions. `xila_file_read_at` and `xila_file_write_at` perform
I/O without moving that cursor. Positional writes use their explicit offset
when the opening was created for append. Pipes and character streams return
`InvalidSeek`.

## Clocks, sleep and randomness

Core's `NativeFormat` handler (`native-dev`) installs the time manager from
`ExecutableContext` and looks for the entropy character device at
`/devices/random/source` before starting a native ELF. Once installed, each
service is reused for subsequent launches. Clock reads before installation and
unsynchronized realtime reads return Unavailable.

`xila_time_resolution(clock, &nanoseconds)` reports the nominal quantization of
`XILA_CLOCK_MONOTONIC` or `XILA_CLOCK_REALTIME`; actual precision can be coarser.
The realtime resolution is the coarser of the clock's synchronization sample and
the monotonic clock, and stays readable while UTC is unsynchronized, when it
equals the monotonic one. An unknown clock or a null output returns
InvalidArgument; no installed time manager returns Unavailable.

`xila_time_sleep(nanoseconds)` waits at least that long on the executor's clock,
which is the monotonic clock unless the host installed a custom one, and is
unaffected by UTC corrections. Inside a coroutine it suspends on the executor's
Embassy timer, so peers on the same executor keep running; zero yields once.
Embassy timers need an executor waker that the `task::block_on` fallback lacks,
so outside a coroutine the call polls the clock instead (and zero returns at
once). Durations are converted to ticks with saturating arithmetic, rounded up
plus one tick, so `UINT64_MAX` waits practically forever instead of wrapping. It
needs no time manager and returns Ok on hosts whose Embassy time driver works
(there is no signal delivery, so nothing interrupts it).

`xila_random_fill(buffer, size)` opens the mounted entropy character device for
the duration of each fill, using the application's VFS permissions. A pending
source suspends the coroutine, and short reads are retried until the buffer is
full. Zero size succeeds without touching the buffer or the source and permits
a null buffer. Native executables still start when the device is absent, and
nonempty fills return Unavailable. A later launch can discover a device mounted
after the first launch. A source that reports no progress returns InputOutput
and other errors map through the usual status translation. After a failed fill,
the buffer contents are unspecified and must not be used.

## Processes and environment

A native process is the Core task that runs a native entry on a stackful
coroutine. There is no second table: the task registry already owns the identity,
the parent link, the user and group, the environment and the join signal, so the
calls read and write it. The host creates the task, either with
`executable::execute` or through `xila_process_spawn`, and hands the entry
`XilaStartup.process`, an opaque `XilaProcess` capability. It is borrowed for the
entry call like the other startup objects and is null for an application that is not
a process, such as a built-in one run through `run_main`; process calls then return
InvalidArgument. Every process call takes the capability explicitly, so nothing is
ambient: a child gets exactly the streams, home directory, arguments and
environment its parent passes.

`xila_process_id` and `xila_process_parent_id` return the Core task identifiers as
`uint32_t`. A process whose parent has exited is adopted by the root task and
reports identifier zero as its parent; Core already does this when a task
unregisters. A task identifier can be reused after the task ends, so hold a
`XilaChild`, not an identifier, to refer to a child.

### Exit

A return from the native entry or a successful `xila_process_exit(process, status)`
ends all threads in the process, including detached ones. The host does not wait
for them or resume their pending operations. The task's join handle reports the
entry's return value or the explicit exit status. Child processes have their
usual independent lifetimes.

The coroutine that calls `xila_process_exit` leaves through `stackful::exit`.
Its native frames are abandoned without unwinding or running destructors. The
main coroutine releases the process's resources on return: the handles the program
opened and did not close, its graphics objects, the granted startup streams, its home
directory and its strings (see [Ownership and cancellation](#ownership-and-cancellation)).
An explicit process exit from any thread stops its peers and releases the same
resources. Unstarted workers and workers that finish release their stacks. Suspended
peers have their frames discarded and their stacks freed when the process's driver
ends, like a cancelled process's. The process heap and mapped image belong to the
loader's task future and are reclaimed after all C execution has ended.

The host invokes native entry points with no owned guards in their callback
frames. The SDK first calls `xila_thread_stop_others(process)` for terminal
cleanup. The driver thereafter polls only that caller, including if a cleanup
operation suspends. The caller must complete process exit without waiting for
a peer. New worker creation returns InvalidState.
A process on a target without stack switching must return from its entry.
Passing another process's capability fails with InvalidArgument.

### Spawn, wait and ownership

`xila_process_spawn(process, directory, path, path_length, &spawn, &child)` starts the
file `path` below the directory capability, which needs the READ right, as a child
of the calling process. The directory plays the role of a search path: a process
cannot name an executable outside the capabilities it holds. The call goes through
`executable::execute`, so the host's format registrations, the execute permission
check and the setuid refusal apply as for any other launch, and the child is an
ordinary Core task: a child of the caller's task, on the caller's executor, running as
the **same user and group**. Setuid files are refused for native images.

`XilaSpawn` carries `size` (set it to `sizeof(XilaSpawn)`) and reserved `flags` (zero),
the child's arguments (no implicit program name), an optional explicit environment,
the three standard streams and an optional home directory. The streams are required,
because Core's `Standard` has no absent stream; pass a platform null device to give a
child nothing. The call duplicates each stream, so the child owns its copies and the
parent keeps its own, and the child gets a copy of the home capability bound to the
child's own task. The call suspends the caller while the host reads the file, then
writes the owned `XilaChild` handle. Failures use the usual statuses: NotFound,
PermissionDenied, ExecFormat for a file that is no image the host can run, and
OutOfMemory when the task table is full. Task manager errors use the canonical I/O
kind mapping: a full task table or no free spawner slot is OutOfMemory, no spawner
available is Unavailable, an unknown spawner is InvalidArgument and a task or thread
that is gone is NoSuchTask. Native children run on the same
single executor, so a child progresses only while its parent is suspended. The
capability must be the calling task's own, as for exit: another process's capability
returns InvalidArgument without effect. A child that fails before its entry runs
(its task or environment could not be set up) exits with status -1, which a program
can also return itself, so a parent that must tell the two apart needs the child to
report success some other way.

`XilaChild` is the capability to wait for one child, and the only way to do it. Only
the process that spawned the child may wait: another process holding the pointer gets
NoChild. `xila_process_wait(process, child, flags, &status)` collects the exit status
and reaps the child; a second wait on the same handle returns NoChild. Inside a
coroutine it suspends on the child's join signal like a blocking file read, so
peers on the executor keep running. `XILA_WAIT_NO_HANG`, or a caller that is not on a
coroutine and so cannot suspend, makes a running child return WouldBlock. Unknown flag
bits are InvalidArgument. The status is the child's `int32_t` return or exit value; a
non-native child's wider status is truncated. The status output is optional and only
written on success.

A child that has exited and was not waited for is a zombie in the only sense Core
has: its task is gone (Core unregisters it before it signals), and its status waits in
the signal owned by its `XilaChild`, a few bytes released by `xila_process_child_close`.
Closing a handle of a running child detaches it: it keeps running, is adopted by the
root task if its parent ends first, and its status is dropped. Null closes succeed. A
parent that ends with unclosed handles has them closed for it, like any other handle it
opened.

### Environment

The environment is the task's own list of `NAME=VALUE` pairs in Core's task manager,
so an empty environment allocates nothing and nothing is reserved per process. A
task copies its parent's environment when it is registered. That is the inheritance
rule: **copy on spawn**. A variable the parent sets later never reaches an existing
child, and the child's changes never reach the parent. `XilaSpawn.environment` instead
gives the child an explicit, complete environment (`NAME=VALUE` strings, a later entry
overriding an earlier one); a null array keeps the copy, and a non-null array with count
zero is an empty environment. The host applies it before the child's entry runs. The
variables go away with the task.

`xila_env_get(process, name, name_length, buffer, capacity, &value_length)` copies the
value without a terminator. A missing variable returns NotFound and leaves the output
alone. A buffer that is too small returns BufferTooSmall, stores the required
length and leaves the buffer unchanged; a null buffer is allowed at capacity zero.
`xila_env_set` replaces any value, and a null value is valid at length zero.
`xila_env_remove` of a missing variable succeeds.
`xila_env_entry(process, index, buffer, capacity, &entry_length)` copies one
`NAME=VALUE` entry without a terminator. Start at index zero and stop at
NotFound, which leaves both outputs untouched. Short buffers follow the
`xila_env_get` convention. Indices can change when the environment is mutated,
so callers must coordinate writes for a consistent pass. This call occupies
slot 51 and shifts the following slots in the development ABI 0.1.

Names are non-empty UTF-8 without `=` or NUL and values are UTF-8 without NUL,
otherwise InvalidArgument. Entries are not
bounded: the memory comes from the system allocator, which follows its usual
failure policy, and applications are trusted not to flood it.

## Native threads and local storage

`xila_thread_spawn(process, stack_size, entry, argument, &id)` creates a local
stackful coroutine. A worker shares the process's task identity, permissions,
heap and executor with main. The driver polls main and the workers as one Core
task using its real executor waker. Drivers keep their existing affinity.
Scheduling is cooperative. A worker starts when the caller suspends and can
yield explicitly through `xila_thread_yield`. A worker that never yields or
waits can monopolize the executor.

A zero stack size selects 4 KiB on bare metal or 64 KiB on Linux. Explicit sizes
are 2–64 KiB inclusive, rounded up to 16 bytes with a 64-byte guard. Size for the
deepest ABI call, debug frames and any interrupts that share the stack. Stacks
and worker records are allocated on demand. Stack allocation and worker-table
growth can return OutOfMemory. Stackful control block allocation follows the
runtime allocator's failure policy.

`xila_thread_current` reports a nonzero ID unique within that process lifetime.
Main has ID 1. A worker returns one opaque pointer-sized result. Only its creator
can join and reap the result. Self-joins return Deadlock, join by another thread
returns NoChild and a missing worker returns NoSuchTask. Any thread in the process
can detach a worker that is still joinable. Detaching discards its result and
reaps it once it finishes. A returning worker's unjoined children are detached
automatically. Completion frees the stack immediately, even while a join result
is pending. `xila_thread_exit(result)` lets a worker finish with a join result by
abandoning its own frames. SDK workers run their TLS destructors before calling it.

The process can create up to eight TLS keys. Each key holds a pointer-sized value
for each coroutine, initially null. Deleting a key invalidates it and leaves
ownership of its values with the application. A new key starts empty, including
when it reuses a deleted key's slot. The host does not call TLS destructors. The
SDK layers POSIX destructors on top, and reserves a separate coroutine-local
`XilaThreadLibc` for errno, strtok and the rand/random family.

The native loader wraps process entries with `drive_process`. Other loaders can
wrap a `ProcessLaunch::run` coroutine the same way. Thread calls without a process
driver, including on targets without native stack switching, return Unsupported.

### Mutexes and condition variables

`XilaMutex` and `XilaCondition` are caller-owned objects with fixed addresses.
Zero initialization gives a plain mutex or an empty condition variable. Use
`xila_mutex_init` with `XILA_MUTEX_RECURSIVE` to select recursive locking.
Only the owner can unlock. A plain self-lock returns Deadlock, and a contended
`XILA_MUTEX_TRY` returns WouldBlock. Final unlock transfers ownership directly
to the oldest queued locker. Destroy while locked or waiting returns ResourceBusy.
The mutex must stay live throughout a condition wait, including reacquisition.

A condition wait enqueues the caller before it releases the mutex, without
a suspension in between. It accepts a mutex held exactly once and reacquires
it before returning. Signal removes and wakes the oldest waiter. Broadcast removes
and wakes all waiters, so the condition can be destroyed immediately afterward.
Callers should recheck their predicate under the mutex. A non-null deadline is absolute signed
nanoseconds on the selected ABI clock. The host samples that clock once and
uses the executor timer for the remaining duration. Realtime requires a
synchronized clock and later UTC adjustments do not change an armed wait.
Timeout returns TimedOut after reacquiring the mutex.

Join and synchronization waits suspend through `suspend_on`. Queue nodes are
stored on waiting stacks. No wait allocates or wakes itself while pending.
A worker must release its locks before ending. Individual worker cancellation and
robust-lock recovery are outside this ABI. Terminal cleanup stops peers permanently
and discards their frames, so the cleanup caller must avoid waiting on locks they own.

## Header and compatibility

The `#[repr(C)] XilaApi` struct in `src/api.rs` defines the slot order and
signatures. All calls return the 32-bit `XilaStatus`. The table starts with
the packed version and total byte size, then one function pointer per slot.
Builds without the `network` feature keep the network slots and return
`XILA_STATUS_UNSUPPORTED` from them; builds without `graphics` do the same for
`xila_graphics_call`.

cbindgen generates `include/xila/application.h` without macro expansion. The
build also parses `XilaApi` with syn to produce the SDK stubs and the zero-based
slot snapshot `sdk/slots.tsv`, plus the bindings in `sdk/rust/xila-sys`. Both
bindings use the status enum generated from the I/O kind catalog. Compile-time
checks compare the Rust guest's type sizes, alignments and field offsets to the
host, including every slot in snapshot order. Opaque handles expose only their
pointer representation. Refresh and verify the committed files with:

```sh
XILA_UPDATE_HEADER=1 cargo test -p xila-abi-application generated_header
cargo test -p xila-abi-application-c-tests
```

ABI 0.1 is pre-release. Rebuild native apps against the matching SDK when the
table or status numbers change. After the first release, append slots for minor
versions and increment the major version for incompatible changes. Keep host object layouts opaque to guests
in both languages.

Rust applications can depend on the standalone `no_std` crate `xila-sys`
and call through `XilaStartup.api`. Its optional runtime supplies an entry
macro, a panic hook, and a process allocator. See [Rust in the SDK
README](sdk/README.md#rust) for the guest build and example.

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
array is null at count zero. `XilaStartup.program_name` carries the executable's
launch name separately and is empty for a linked entry. `XilaStartup.process` is the process capability, null
for an application that is not a process.

The host lends the startup data, strings and initial capabilities for the call.
The app may close its own file and directory objects. It must leave borrowed
objects open and release access when the entry returns. The Rust helper drops
the startup resources when the entry returns. The caller supplies the process
heap and owns its teardown.

## SDK startup

Applications written in C normally use the published [SDK](sdk/README.md): picolibc
and a POSIX layer on top of this ABI, with `main` as the entry point. The text below
is the layer underneath, for images that do not use the C library.

A separately linked C image defines `int32_t xila_main(const XilaStartup *)` and
links `sdk/crt0.c` and `sdk/stubs.c`, using `-Iinclude/xila -Isdk` and including
`stubs.h`. crt0 also carries the `XILA` ABI note the loader requires. The host calls `_xila_start` once per image. crt0 checks the startup
and table sizes and the ABI version, installs the image-local table pointer,
then calls `xila_main`. Before startup and after `xila_main` returns, all stubs
return `XILA_STATUS_UNAVAILABLE`. Once the entry returns, the host ends all remaining workers. Invalid startup returns -1. The [native loader](../../native_loader/README.md) maps
such an image and calls `_xila_start`.
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
not own, an interior pointer or a block that was already freed, including through
a pointer that a moving resize left stale. A foreign-heap pointer is missed only
if the two heaps' keys collide, which is unlikely.

Each block starts with a 16-byte header on 32-bit targets (Arm and RV32) and
32 bytes on 64-bit targets: previous and next links, the size and an owner tag.
The doubly linked list gives constant-time free and resize. The tag is a
per-heap key XOR the block address. Free clears it, and so does a moving
resize before it releases the old block. Free and resize check
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
an unfinished coroutine. The same holds for the host state of a run
(`HostState`): the `XilaStartup` block, the process capability and the
arguments and streams they point into. The loader allocates it beside the heap
and passes the coroutine entry only a pointer to it, so the entry call keeps no
host state in its frames and `xila_process_exit` finds none there. The
resources are released by whichever ends the run first: the entry returning,
`xila_process_exit` before it abandons the coroutine, or the host cancelling the
process (`HostState::release`); releasing is idempotent, so a later release finds
nothing. A loader that never starts the run releases the state itself. `run_main`
is the exception: it is for a linked program on the caller's own stack and
keeps the state in its frame. See [Ownership and cancellation](#ownership-and-cancellation).

## Ownership and cancellation

A native process ends in one of three ways: its entry returns, it calls
`xila_process_exit`, or the host cancels it by dropping its task future while its
coroutine is suspended (executor teardown, or a dropped spawner: the task manager has no
cancel call, and a future preemptive kill would take this path too). In every case the
host releases everything the process holds, in the same order, from state it owns, and
runs none of the program's code: a cancelled coroutine's frames are discarded, never
unwound (see `stackful`'s Cancellation).

That works because **no frame owns a resource across a suspension**. Where each one lives:

| What the process holds | Owner | Released by |
|---|---|---|
| Image and process heap (and the blocks C allocated) | the process allocation, owned by the task future | the loader, with the allocation |
| Startup streams, home directory, arguments, program name, startup block | `HostState`, in that allocation | `HostState::release` |
| Files, directories, listings, children, sockets the program opened (`File`, `Directory`, `DirectoryIterator`, `Child`, `Tcp`, `TcpListener`, `Udp`) | the process's handle registry, in `HostState` | `HostState::release`, newest first |
| Graphics objects and the identity file pickers act with (the task) | `Resources.graphics`, in `HostState` | `HostState::release`, then dropping the state |
| A file the call itself opens (the entropy source), a socket being connected or bound, the duplicated streams and the request of a spawn, the handle a wait holds | the suspended future, passed to `suspend_on_cancellable` | stackful, which drops the future in place on cancellation |
| The coroutine stack | the coroutine's control block | stackful, which frees it on cancellation |
| Worker threads' stacks | their coroutines, owned by the process's driver | the same: they are cancelled when the driver ends |

A host that cancels a process drops its coroutine first (which frees the stack and drops
the cancellable futures), then calls `HostState::release`, then frees the state, the heap
and the image. The native loader does this in that order on every path, including a
program that never started and a spawn the task manager refused. Releasing is idempotent,
so it also runs after a normal end, where it finds nothing to do except the handles the
program forgot to close: those are closed too, once, and the standard streams are closed
whether or not the program returned.

**Handles.** An object that `into_raw` hands to a program is boxed behind a link that
lists it in the registry of the process whose coroutine opened it, and `from_raw` (every
`xila_*_close`) unlinks it. A worker thread registers in the same registry as the main
thread. Outside a coroutine nothing registers: the caller keeps the object, as a host that
builds a `Directory` capability to grant. Closing goes through the same `xila_*_close`
function as the program's own call, and each close runs while the object is still
registered (a file, a listing and a socket are closed in place, then unlinked), so a
cancellation that lands while a close waits for a lock leaves the object to the host,
which closes it again. A UDP socket is the exception: its close consumes it, so a
cancellation in the middle of it leaks that one socket.

**Cost.** The registry is one pointer per process, and a link is three pointers in front
of each handle: 12 bytes on a 32-bit target (16 for files and listings, whose contents
are 8-aligned), with no allocation, no growth policy and no failure mode. The standard
streams and the home directory carry no link. On Arm and RV32 the state grows from 384 to
392 bytes.

**Rules for the ABI.** A call that opens an object registers it before it can suspend
again, or keeps it inside the future it suspends on and passes that future to
`suspend_on_cancellable`; it never leaves one in its own frame while it waits. A future
passed to `suspend_on_cancellable` borrows nothing the host frees before the coroutine is
dropped. Native code is trusted, so none of this guards against a program that closes a
handle twice or passes another process's handle.

**What a cancellation still leaves behind.** A call suspended inside the VFS or a driver is
not dropped unless it is one of the cancellable suspensions above, so a device keeps whatever
registration it made for a pending read or write: a clone of the dead task's waker, which is
harmless to wake. The VFS drops such a call's `cancel_operation` guard only with the future,
and that guard touches the file the call borrowed; a stopped worker's pending read outlives
the process's exit by one driver drop, by which time the exit has closed and freed the file,
so those futures stay non-cancellable. A close that was waiting for the network stack when
the process was cancelled loses that one UDP socket (above). No ABI call holds a lock guard
across a suspension.

## Optional graphics (`graphics`)

Graphics is disabled by default. With the `graphics` feature (the facade's `graphics`
feature enables it through `xila-executable`), `xila_graphics_call(process, function,
words, count, result)` runs one call of Core's graphics executor for the process. It is
the native transport of the service the Wasm host also uses: the executor
(`graphics::call`) owns the graphics lock, the per-process object table and cleanup,
the event queue and the Xila extension calls; the transport only says how a pointer
becomes memory. Here pointers are host pointers, read in place because native code is
trusted. Strings are scanned with a bound (`strnlen` style), so the executor's limits cap
how much memory an unterminated string makes the host read. Beyond those limits the only
check is that a buffer's length can be a slice at all (at most `isize::MAX`, no wrap). `function` is a call ID from the catalog, `words` the call's `count` (at most 7)
arguments, and `result` receives a pointer-sized word or a 32-bit value, depending on the
call. The SDK generates a function for every call from the declaration that generates the
executor, with the names of the Wasm guest API (`xila/graphics.h`, `xila_sys::graphics`);
see the [SDK README](sdk/README.md#graphics).

* **Per-process state.** The call's object table (`graphics::call::State`) belongs to the
  process's host-owned `Resources`, off the coroutine stack, and is allocated by the
  process's first call: a process that never draws holds one null pointer
  (`Option<Box<State>>`; 4 bytes on a 32-bit target). It is deleted, with the LVGL objects
  it names, when the process ends through a return from the entry, `xila_process_exit` or a
  cancellation, after the handles the program opened and before its standard streams are
  closed. The state, with the task identity file pickers act with, is dropped then. If the
  coroutine is cancelled while it waits to delete, the table stays in the host state for
  the host to finish, as for every other resource.
* **The lock.** A call waits for the graphics lock with `Manager::lock().await` inside
  `suspend_on`, so a contended lock suspends the process, never the executor, and runs
  the call with `graphics::call::execute_locked` once it has the lock. It never awaits
  while holding it. Outside a coroutine the wait falls back to `block_on`, like every
  other bridge.
* **Status.** Wrong handles, arguments and values are `InvalidArgument`, a string, buffer
  or table over its limit is `OutOfMemory`, no graphics manager in the executable context
  (`ProcessHost::graphics_manager`) is `Unavailable`, and a build without the feature is
  `Unsupported`. A process without the capability (a linked entry) gets `InvalidArgument`.
* **No raw LVGL.** `lv_*` functions are not part of the ABI. They would bypass the
  lock and the ownership above, so the objects of an exited or cancelled process would
  leak and a process could delete another's. The call table is the whole surface; an
  `lv_*` layer can be built on it later.

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
