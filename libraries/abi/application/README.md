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
Read requires READ; write, create, truncate and append require WRITE. Native
code remains trusted: pointers must be live, calls on each object serialized,
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

- `xila_abi_version`: packed major/minor version, initially 0.1.
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

Directory listing, statistics, rename/remove, tasks/threads, networking and graphics
are not included. Program entry and handing initial capabilities to an application
belong to the future native loader ([Core issue #334](https://github.com/xila-project/core/issues/334)).
A POSIX compatibility layer can be built above this native interface later.
