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
canonicalized. Empty file paths are invalid. Child rights can only decrease.
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

- `xila_abi_version`: packed major/minor version, currently 0.2.
- `xila_directory_open`, `xila_directory_close`: attenuated path capabilities.
- `xila_file_open`, `xila_file_close`: typed owned file objects.
- `xila_file_read`, `xila_file_write`: direct buffer I/O.
- `xila_file_seek`, `xila_file_flush`: cursor and persistence operations.
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

Additive ABI changes bump the minor version; breaking changes bump the major
version. Never reuse or renumber constants or status values. Rust object layouts
are private and must never be inspected or allocated by C callers.

## Future scope

Directory listing, statistics, rename/remove, networking and graphics
are not included. Program entry and handing initial capabilities to an application
belong to the future native loader ([Core issue #334](https://github.com/xila-project/core/issues/334)).
A POSIX compatibility layer can be built above this native interface later.

## Native threads and synchronization

`xila_thread_spawn` starts a dedicated OS thread on Linux.
The nullable `XilaThreadOptions` has a leading byte `size`; supply at least
`sizeof(XilaThreadOptions)` (larger future structures are accepted). Stack size
zero uses the OS default; nonzero is a requested size subject to the OS minimum
and alignment. The optional name is copied from a UTF-8 pointer/length pair;
embedded NUL is invalid. The argument is passed unchanged and is not copied or
freed. Entry must return normally; unwinding across C is unsupported.

Join waits without spinning, writes the signed entry result and consumes the
join token. Invalid arguments and self-join leave the token live; self-join
returns Deadlock. An unexpected Rust thread panic consumes the token and returns
Unavailable without writing the result. Detach consumes only the token: the
thread continues and its stack/runtime resources are released on exit. The
caller must keep entry code and argument storage live until completion, including
for detached threads. There is no cancellation, priority or affinity API.

These are native threads within the caller's application, **not new registered
Xila tasks**. Authority remains attached to the explicit File/Directory
capabilities supplied in the argument, retaining the originating task identity;
spawn does not grant root authority or manufacture a new task identity. The
caller must keep that originating task and its capabilities alive until all
threads using them finish. File/directory access still needs serialization.
Ambient task-manager APIs that expect an Embassy task waker must not be called
from native threads. No task manager lookup or thread-local authority table is
introduced by this ABI.

`xila_mutex_create` accepts zero or `XILA_MUTEX_RECURSIVE`; unknown bits fail.
Ownership is per native OS thread. Lock sleeps on contention; a nonrecursive
self-lock returns Deadlock, while try_lock returns WouldBlock. Recursive depth
overflow returns Overflow. Unlock by a non-owner returns PermissionDenied.
Destroying a locked mutex returns Busy without consuming it. Destroy requires
external quiescence: no concurrent callers or waiters may retain the object.

`xila_condvar_*` names avoid the incompatible legacy `abi_definitions`
`xila_condition_variable_*` exports. Wait requires the caller to hold its mutex
exactly once (including recursive mutexes); otherwise it returns InvalidArgument
without releasing anything. Wait atomically releases the mutex and parks, then
reacquires it before returning Ok or TimedOut. Always test a protected predicate
in a loop: spurious wakeups are permitted and notifications are not saved.
All waits on a condition variable must use the same mutex. Signal wakes one
waiter; broadcast wakes all. Change the predicate under that mutex. Timeout
measures the notification wait, not the subsequent mutex reacquisition, so the
call can return later than its deadline. An unrepresentable deadline returns
Overflow before releasing the mutex. Sleep uses the OS scheduler and lasts at
least the requested nanoseconds; yield is an OS scheduling hint.

Blocking calls must run on a dedicated native thread, **not within an Embassy
executor poll**. Parking an executor thread would stall every future assigned to
it. Other executor/OS threads continue normally. The native loader must arrange
this entry context; existing Platforms executables have async Rust entry points
and are not automatically migrated by this change.

### Embedded boundary and investigation

Task Manager allocates Embassy task futures and selects among registered
spawners by task count. Spawners are cooperative executors, not independent C
stacks. Platforms' std driver can start another executor on an OS thread, but
there is no portable embedded stackful/preemptive thread primitive. In addition,
`task::block_on` delegates to Embassy's busy-loop implementation: using it for
locks, joins or sleeps would burn CPU and can deadlock a single executor.
Existing ABI-definition mutexes/rwlocks only probe critical-section-protected
state; condition variables and semaphores are unimplemented. Embassy async
primitives cannot suspend an ordinary synchronous C stack, so none is reused as
a blocking adapter and there is no dependency on `abi_definitions`.

On other targets (including bare metal, Wasm, macOS and Windows), these calls validate arguments
and return Unsupported, leaving outputs untouched and allocating nothing. This
is intentionally **not embedded threading support**. Supporting blocking native
threads there requires a platform stack/context-switch or RTOS backend; an async
spawn wrapper or repeated yielding is insufficient. No cooperative C continuation
or shared-stack emulation is claimed. This boundary keeps unsupported targets'
RAM/CPU cost at zero rather than silently starving their executors.

### Cost

Measured with `size_of` in the x86_64 Linux Rust tests:

| Object | Bytes | Additional cost |
| --- | ---: | --- |
| Thread | 24 | OS thread stack, runtime bookkeeping and join packet |
| Mutex | 32 | One Box allocation at create |
| ConditionVariable | 4 | One Box allocation at create |
| ThreadOptions | 32 | Caller storage; not retained |

Synchronization uses standard OS-backed mutex/condition primitives. The ABI
allocates each synchronization object once; lock, unlock and wait allocate no
ABI storage and do no logging. Linux's futex-backed primitives need no extra
heap storage. Other host backends are not enabled until their allocation behavior
and cost are verified (some standard-library backends allocate lazily on first
use). Calls from foreign, non-Rust-created OS threads are outside the contract:
Rust thread-local initialization can allocate on their first call. Both the
native loader entry and threads spawned here must use Rust-created OS threads.
Object sizes are private and platform/toolchain dependent. No handle registry, waiter vector, per-wait Box or per-thread executor is added. Counting
semaphores, rwlocks, robust recovery, fairness and priority inheritance are not
provided. New status numbers are TimedOut=20, Deadlock=21, Busy=22; contention
reuses WouldBlock=9 and missing backends reuse Unsupported=10.
