# Xila native SDK

Compile C against picolibc and a POSIX layer, or Rust against the raw
`xila-sys` bindings. Link a static PIE and run it on Xila without rebuilding
the firmware. Core's CI builds one archive per target
and uploads it as a workflow artifact:

    xila-sdk-<abi>-<target>.tar.zst      for example xila-sdk-0.1-thumbv7em-none-eabihf.tar.zst

| Target | Class | Float ABI | Compiler runtime |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | the host Platforms runs on Linux | none | none needed |
| `thumbv7em-none-eabihf` | Cortex-M4F | hard | Arm GNU toolchain's `libgcc.a` |
| `riscv32imac-unknown-none-elf` | ESP32-C6 class RV32IMAC | soft (`ilp32`) | bare-metal RISC-V GCC's `libgcc.a` |

Xtensa (`xtensa-esp32s3`) is an addition to the build tables, not a redesign.
The RISC-V images do no gp relaxation: the link defines no `__global_pointer$`
(and never passes `--relax-gp`), and `check_elf.py` rejects an image that defines
it, because the image's gp is not the firmware's. Call relaxation (pc-relative
`jal`) stays on: it is valid in a position-independent executable. They cannot
run on the Linux host loader, so CI validates them structurally with
`check_elf.py`, like Cortex-M.

## Layout

```text
xila-sdk-0.1-<target>/
  sdk.json                  target, ABI, picolibc pin and options, compiler
  COPYING.picolibc          picolibc's licence (BSD); LICENSE-xila.md is Core's (Apache-2.0)
  include/                  picolibc headers, xila/application.h, xila/stubs.h,
                            xila/posix.h, sys/random.h, sys/socket.h, pthread.h
    posix/stdlib.h          wrapper for the SDK's getenv declaration
  lib/
    crt0.o                  _xila_start: checks the startup, installs the API table
    libxila.a               indirect-call stubs for every XilaApi slot
    libxila_posix.a         the POSIX layer; also defines xila_main, which calls main
    libc.a, libm.a          picolibc (libm.a is empty: the math library is inside libc.a)
    xila-static-pie.ld      the loader's linker script
  rust/xila-sys/             standalone Rust crate
    Cargo.toml, Cargo.lock
    src/                    raw bindings and optional entry runtime
    build.rs                supplies the guest linker arguments with the link feature
    xila-static-pie.ld      copy of the loader's linker script used by build.rs
  cmake/xila-toolchain.cmake
  share/xila/example/hello  the example below
```

## Building an application

Needs `clang`, `lld` and CMake. On Cortex-M and RISC-V the 64-bit and soft-float
helpers come from a compiler runtime: install the Arm GNU toolchain
(`gcc-arm-none-eabi`) or the bare-metal RISC-V GCC (`gcc-riscv64-unknown-elf`,
which provides the `rv32imac`/`ilp32` `libgcc.a`; it does not need newlib), or pass
`-DXILA_BUILTINS_LIBRARY=<libgcc.a>`.

```sh
tar --zstd -xf xila-sdk-0.1-thumbv7em-none-eabihf.tar.zst
cmake -S share/xila/example/hello -B build \
  -DCMAKE_TOOLCHAIN_FILE=$PWD/xila-sdk-0.1-thumbv7em-none-eabihf/cmake/xila-toolchain.cmake \
  -DCMAKE_BUILD_TYPE=MinSizeRel
cmake --build build        # build/hello.elf
```

Your own project is an ordinary `CMakeLists.txt` with `add_executable`. The
toolchain file compiles with `-D_GNU_SOURCE -fPIE -ffunction-sections`, links with
`ld.lld` directly (`-pie --no-dynamic-linker -Bsymbolic`, the SDK linker script, crt0,
then `libxila_posix`, `libxila`, `libc` and the compiler runtime as one group) and
names executables `*.elf`. `CMAKE_EXE_LINKER_FLAGS` takes `ld.lld` flags, without
`-Wl,`. Constructors, `__thread` variables and `dlopen` are not supported: the loader
rejects an image with `DT_INIT`, a non-empty `DT_INIT_ARRAY` or `PT_TLS`.

For custom C builds, put `<sdk>/include/posix` before `<sdk>/include` on the
system include path. The SDK's `<stdlib.h>` wrapper includes picolibc's header
and binds `getenv` to a symbol the compiler treats as an ordinary function.

Copy `hello.elf` to a filesystem the platform's native format reads (Core's
`NativeFormat`, the `native-dev` feature) and execute it. `main(int argc, char **argv)`
receives the arguments. For a native executable, `argv[0]` is its launch name
(the executable's basename when launched through `executable::execute`). A linked
entry with no process capability gets an empty `argv[0]`.
Returning from `main` uses `exit` and performs the cleanup described below.
Both `exit` and `_exit` close the descriptors the program left open. `_exit`
discards buffered output and skips exit handlers.

### The example

`share/xila/example/hello/hello.c` exercises `printf`/`snprintf`, `malloc`, files
(`open`, `read`, `write`, `lseek`, `pread`, `fstat`, `rename`, `unlink`), directories
(`mkdir`, `chdir`, `getcwd`, `opendir`, `readdir`), `fopen`/`fgets`/`fprintf`,
clocks, `nanosleep`, `getentropy` and `getenv`. Core's test builds it with this
toolchain file and runs it through the VFS, the executable dispatch table, the
native loader and the ABI table (`libraries/executable/tests/sdk.rs`).

## Rust

Add `xila-sys` to your application's `Cargo.toml` and replace `<core-commit>`
with the full commit SHA for the Core revision you build against.

```toml
[dependencies]
xila-sys = { git = "https://github.com/xila-project/core.git", rev = "<core-commit>", features = ["link"] }

[profile.dev]
panic = "abort"

[profile.release]
panic = "abort"
```

You can also use a path dependency on `rust/xila-sys` from the SDK archive.
The raw bindings and runtime API can be checked with Rust 1.97. The crate
has no dependencies and exposes the same startup record,
opaque capabilities, ABI constants and table as the C header. Calls through the
table are unsafe. Follow the ABI's buffer, lifetime and ownership rules,
especially for the borrowed startup streams. `Status` preserves every 32-bit
value, including status codes introduced by later hosts. Use `Mutex::ZERO` and
`Condition::ZERO` for initial synchronization storage.

The `link` feature enables `runtime`, which provides `xila_sys::entry!(main)`.
The entry macro emits `_xila_start` and the ELF ABI note, checks both sizes and the table
version, then calls your `unsafe fn main(*const xila_sys::XilaStartup) -> i32`.
The result is the process exit status. During entry, `xila_sys::runtime::api()`
returns `Some(&XilaApi)`, and it returns `None` outside entry. Define a
`#[panic_handler]` and call `xila_sys::runtime::panic_abort()` to report
`panicked` on standard error and exit with status 134. If you use
`alloc`, you can install `xila_sys::runtime::ProcessAllocator` as your
`#[global_allocator]`. The host reclaims the guest heap on exit.

Save this as `.cargo/config.toml` in the application:

```toml
[unstable]
build-std = ["core", "alloc", "compiler_builtins"]
build-std-features = ["compiler-builtins-mem"]

[build]
rustflags = ["-C", "relocation-model=pic", "-C", "linker=rust-lld"]
```

Use the `nightly_version` pinned in Core's [`mise.toml`](../../../../mise.toml)
and install its `rust-src` component. Build a `#![no_std]`, `#![no_main]`
binary with an explicit target so the PIC flags also apply to the rebuilt
sysroot while build scripts still use the host configuration.

```sh
XILA_NIGHTLY='<nightly_version from the pinned Core revision>'
rustup toolchain install "$XILA_NIGHTLY" --profile minimal --component rust-src
cargo +"$XILA_NIGHTLY" build --release --target thumbv7em-none-eabihf
```

The supported targets are `x86_64-unknown-linux-gnu`, `thumbv7em-none-eabihf`
and `riscv32imac-unknown-none-elf`. Cargo's nightly `build-std` is needed
because the prebuilt bare-metal `core` is not position-independent.
You can omit `alloc` if your application never uses it. A future dedicated
Xila target with `std` would start as tier 3 and would still need a source
build of its sysroot.

On the guest nightly, `link` supplies the linker script and all Xila
link arguments, including the target's data alignment. It uses rustc's
nightly support for forwarding link arguments from a library to its
consumer. Direct GNU-style linking uses the `rust-lld` bundled with Rustup,
so the application needs no system LLVM install, custom linker script,
C startup object, or libc.

Enable only `runtime` when building host tests or doctests that use the runtime
API. Cargo unifies dependency features within a build, so keep host tests and
guest builds that enable `link` in separate Cargo invocations.

From a Core checkout on Linux x86-64 with Rustup and `llvm-size`:

```sh
mise run sdk:rust-example
```

This builds `sdk/rust/hello` for Linux x86-64, Cortex-M4F and RV32IMAC, checks
all three ELFs with `tools/sdk/check_elf.py`, and runs the Linux image through
`executable::execute`. The program reads the monotonic clock, exercises the
process heap by growing a `Vec` and resizing a 64-byte-aligned block, writes to
its borrowed stdout and returns status 73. A second launch checks the panic
diagnostic and exit status.

## How it maps onto the native ABI

* **No ambient authority.** Core's ABI takes a directory capability and a pointer plus
  a length for every path. The layer keeps an `int` file-descriptor table and a
  current-directory string, normalizes paths lexically (`.` and `..` never reach Core;
  `..` does not look through symbolic links), and resolves absolute paths against the
  *root capability*, which today is `XilaStartup.home`. The host grants it for each
  launch (`executable::Launch::with_home`, or the home of a spawn request); a program
  launched without one gets no filesystem authority and its path calls fail with
  `EACCES`.
  When the ABI adds `root` and `cwd` handles (core#717) the layer prefers them.
* **Descriptors.** Slots 0-2 are the startup streams; `close(0)` detaches without
  closing host-owned objects. The table is on the process heap: eight slots (8 bytes
  each on 32-bit targets) at first, doubling to at most 256.
* **Errors.** `errno` comes from one table keyed on `XilaStatus` wire values
  (`posix/status.c`). Unknown statuses become `EIO`.
* **Heap.** `malloc`, `calloc`, `realloc` and friends are the process heap
  (`xila_heap_*`); picolibc's own allocator is not built. Alignments above
  `XILA_HEAP_ALIGNMENT` fail.
* **Environment.** `getenv`, `setenv` and `unsetenv` use the process capability
  and see inherited or explicitly supplied launch values. Names and values
  follow the ABI's UTF-8 rules. `getenv` returns NULL for a missing name
  without changing `errno`. It can return NULL with `ENOMEM` if allocation
  fails or with `ENOMEM` or `EINVAL` if adoption of an assigned table fails.
  `setenv` honors its overwrite flag and `unsetenv` succeeds for an
  absent name. Mutations reject a NULL or invalid name with `EINVAL`.
  Without a process capability, `getenv` returns NULL and valid mutations fail
  with `ENOSYS`. Picolibc's `putenv` copies the assignment and calls `setenv`.

  Each lookup checks the ABI, so it sees direct ABI updates. The SDK keeps one
  NUL-terminated cached value per name. A lookup of that name that observes a
  changed or removed value can invalidate the earlier pointer, as can a POSIX
  mutation of the same name or process exit. Lookups and mutations of other
  names preserve the pointer. A long lookup may briefly allocate a buffer to
  compare with the cached value. The libc lock serializes the POSIX calls
  across coroutines. Coordinate same-name pointer use with those calls.

  `environ` (declared in `<xila/posix.h>`) points to a NULL-terminated array of
  `NAME=VALUE` strings. It reflects the launch environment and subsequent
  changes made by `setenv`, `unsetenv` and `putenv`. It starts empty if the program
  has no process capability. Treat the array and strings
  as read-only and coordinate iteration and assignments with POSIX accessors.
  A program may assign a different NULL-terminated table to `environ`, or
  assign NULL for an empty environment. With a process capability, the next
  environment accessor replaces the host environment and publishes an SDK-owned
  table. Entries without a nonempty name followed by `=` or with invalid UTF-8
  are skipped. For duplicate names the first valid entry wins. Keep the supplied array and strings
  alive until adoption succeeds. The program retains ownership of both.
  Coordinate adoption with direct task or ABI writes to the environment.

  The SDK retains one previous table after adoption. Save `environ` before
  assigning a temporary table and restore it before adopting another table.
  Restoring the retained table reinstalls its array as is, without UTF-8
  validation or duplicate removal, and retains the outgoing table. Writing your
  own pointers into slots of an SDK-owned array is unsupported.

  The SDK never returns an array it built to the heap once `environ` has
  exposed it. An array that leaves the environment loses its strings, reads as
  empty (`array[0]` is NULL) and joins a small pool that later tables reuse, so
  memory is bounded by the peak number of tables in use, not by the number of
  adoptions. Restoring a table saved before the most recent adoption is
  therefore safe but not supported: the next accessor adopts whatever that array
  holds, which is empty unless a later table reused it, in which case it is that
  table's current contents. A mutation of the current table can invalidate
  saved pointers to its strings, and can change its entries in place or move
  them to another array. Removing the last entry leaves the current array in
  place with a NULL first slot.

  Compile with `<stdlib.h>` from the SDK. Its `getenv` declaration carries an
  asm-label wrapper that stops the compiler from assuming `environ` is unchanged
  across the call. A program that declares `char *getenv(const char *);` itself,
  or is built without the SDK's `include/posix` path, bypasses that wrapper, and
  the compiler may then fold comparisons on `environ` after `getenv`.

  To observe direct task or ABI changes, use `getenv` or enumerate through
  `xila_env_entry`.
  Referencing `environ` pulls in the archive member that overrides the SDK's
  hidden weak startup and accessor hooks. The startup hook builds the table
  before `main`. Each mutation reserves any needed memory before updating the
  host. Accessor-only programs retain the default hooks and use zero RAM for
  this table.
* **Process IDs.** `getpid` and `getppid` read the current and parent process IDs
  from the native process capability. A process adopted by the root task has
  parent ID zero. Without a process capability, `getpid` returns 1 and
  `getppid` returns 0. The SDK provides no user or group ID wrappers such as
  `getuid`, `geteuid`, `getgid`, `getegid`, `getgroups`, `setuid` or `setgid`.
* **stdio.** `stdin`, `stdout` and `stderr` are descriptors 0-2 through picolibc's
  buffered POSIX streams, each in its own object, so a program that never reads
  `stdin` carries neither its `FILE` nor its buffer. `stdout` is line buffered (128
  bytes by default; override with `-DXILA_STDOUT_BUFFER=n` when building the SDK);
  `stderr` is unbuffered, so each write reaches the host when it is made. `printf` and `scanf` are integer-only by default;
  configure with `-DXILA_PRINTF_FORMAT=double` (or `float`, `long-long`, `minimal`)
  to link picolibc's other variants. Floating point costs several KB of flash.
* **Coroutine-local libc state.** The SDK keeps `errno`, `strtok` and the
  `rand`/`random` seed in the calling coroutine. Other picolibc state that
  depends on ELF TLS, including `rand48`, `gmtime` and `localtime` result
  buffers and the implicit multibyte conversion state, remains process-wide.
  Use the reentrant variants or caller-supplied state when these calls span
  yields. Picolibc is built with `single-thread=false` and `stdio-locking=true`.
  Its global and per-stream locks are ABI mutexes. ELF TLS remains disabled.
  Applications serialize descriptor operations that share an underlying handle,
  changes to the working directory, and `fflush(NULL)` against open and close
  of streams.
* **Exit.** With no live workers, `exit` runs the `atexit`, `on_exit` and
  `__cxa_atexit` handlers in reverse order and flushes stdio. With live workers,
  it stops all peers, skips exit handlers, closes descriptors outside stdio,
  flushes streams it can acquire, then closes the descriptors backing those
  streams. Streams owned by a stopped thread skip the flush. `_exit` performs the
  descriptor sweep without handlers or flushing. Main returns through `xila_main`
  and a worker reports status through `xila_process_exit`. See the ABI README for
  resource lifetimes. Picolibc's own
  exit-time machinery is replaced because it cannot run here: its handler table runs from
  a `.fini_array` entry and its stdio flush registers itself from a constructor, and
  that flush also forgot each stream it flushed, so a program that called `fflush(NULL)`
  lost its later writes at exit. The build removes those archive members from `libc.a`
  (`DROPPED_MEMBERS` in `tools/sdk/build.py`) and the layer defines the replacements
  (`posix/onexit.c`, `posix/exit_flush.c`). Members are named exactly, per target:
  Arm's `interrupt.c.o` (vector table, no constructor) stays, x86-64's (an interrupt
  descriptor table set up by a constructor) goes. The build also scans every member of every
  SDK archive and fails on any `.init_array`, `.fini_array`, `.preinit_array`, `.ctors`
  or `.dtors` section, so no future picolibc update can hand an application a
  constructor.
* **Stack protector.** The toolchain file builds without it, to save flash and RAM;
  objects compiled with `-fstack-protector`, `-fstack-protector-strong` or
  `-fstack-protector-all` link against `posix/stack_protector.c`, which defines
  `__stack_chk_guard` and `__stack_chk_fail`. The guard is seeded from `getentropy` by
  `xila_posix_initialize` (its lowest byte is NUL; if the host has no entropy a fixed
  value stays), before any protected function runs. On corruption the process writes
  `*** stack smashing detected ***` to descriptor 2 and ends with status 134 through
  `xila_process_exit`, like `abort`: no handler runs and no stream is flushed (the
  buffers may be corrupt), the descriptors are closed, and the coroutine is abandoned
  without returning through any of its frames. It does not use `_exit`'s `longjmp`,
  whose `jmp_buf` lives on the stack that was just smashed; only a host that grants no
  process capability falls back to `_exit(134)`. The host keeps the capability, the startup
  block and the stream behind descriptor 2 off the main thread's coroutine stack, so an
  overflow that stays inside that stack does not reach them. A worker thread's own state
  (its thread identifier and the pointer to its process group) still sits on the worker's
  coroutine stack, so an overflow there that reaches the top of that stack can still break
  the exit (#778). An image
  that does not use the protector does not link the file. picolibc's own member seeds the
  guard from a constructor, so it is removed from `libc.a`.
* **Directories.** Reading a directory stream gives `d_ino` a stable nonzero 64-bit
  value (FNV-1a) derived from the directory's path and the entry's name (the iterator
  reports no inode; it is not `st_ino`). `open` of `/` or `.` read-only gives a directory
  descriptor (`fstat`, `openat` base); with write access, or `O_DIRECTORY` with write
  access, it fails with `EISDIR`.
* **Path escapes.** `openat` with a directory descriptor cannot leave that directory;
  the error is the errno of Core's `PATH_ESCAPES_ROOT` status (`EACCES`, as the
  `io_error` catalog projects it).

## POSIX subset

Provided: `open`, `openat`, `creat`, `close`, `read`, `write`, `pread`, `pwrite`
(positional I/O with the cursor preserved), `lseek`, `fsync`, `fdatasync`, `ftruncate`, `fstat`, `stat`, `lstat`,
`fstatat`, `mkdir`, `mkdirat`, `unlink`, `unlinkat`, `rmdir`, `rename`, `renameat`,
`opendir`, `readdir`, `rewinddir`, `closedir` (no `.` or `..` entries), `getcwd`,
`chdir`, `isatty`, `clock_gettime`, `clock_getres` (`CLOCK_REALTIME`,
`CLOCK_MONOTONIC`), `nanosleep`, `sleep`, `usleep` (not interruptible: there are no
signals), `time`, `gettimeofday`, `getentropy`, `getrandom`, the `malloc` family, C
`stdio`, `exit`, `_exit` and `abort`. Mode bits passed to `open` and `mkdir` are ignored.

The descriptor table allocates the lowest free fd for `dup`, `open` and
`pipe`. `dup2` replaces a specific fd. File duplicates share their VFS cursor,
including across `read`, `write` and `lseek`. Directory duplicates share
the path capability's authority, and directory streams have independent
`readdir` positions. `fcntl` supports `F_DUPFD`, `F_DUPFD_CLOEXEC`,
`F_GETFD`, `F_SETFD`, `F_GETFL` and `F_SETFL`. The close-on-exec bit is
per descriptor. `F_GETFL` reports the access mode and supported status flags.
`F_SETFL` changes `O_NONBLOCK` and `O_APPEND` for file handles, including
every duplicate of the same opening. Directory descriptors report `O_RDONLY`
and reject `F_SETFL`.

`pipe2` accepts `O_NONBLOCK` and `O_CLOEXEC`. Pipes hold 512 bytes.
Writes of up to `PIPE_BUF` (512 bytes) are atomic. A nonblocking atomic write
returns `EAGAIN` without queuing bytes if the whole write does not fit.
Writes larger than `PIPE_BUF` can return a partial count even in blocking
mode, a deviation from POSIX. Blocked pipe writers have no FIFO fairness
guarantee. Core's pipe opening limit also applies to pipe duplicates.

`poll` supports read/write readiness, error, hangup and invalid descriptors.
Pipe output readiness requires space for an atomic write. A write-only
regular file reports output readiness when requested. Character devices
without a readiness probe always report the requested directions allowed by
the opening. Poll never waits for readiness on these devices. When the
fallback supplies every reported event, poll yields once before returning.
For example, polling a device and a pipe can report the device as readable
while the pipe is empty. A blocking read then waits on the device. Use
`O_NONBLOCK` and handle `EAGAIN` in a poll loop. Drivers should implement
precise readiness. Applications Terminal and the Platforms std console are
follow-ups. Negative descriptors are ignored. A poll wait snapshots its descriptors at
entry. Applications should coordinate closing or replacing a descriptor with
other threads that are waiting on it. `ioctl(fd, FIONREAD, &queued)` writes the
number of bytes queued in a pipe to an `int`. Other ioctl requests fail with
`ENOTTY`.

The SDK provides `pthread_create`, `pthread_join`, `pthread_detach`, `pthread_self`,
`pthread_equal`, `pthread_once`, stack and detach attributes, TLS keys with up to
four destructor passes, plain and recursive mutexes, condition variables including
timed waits, and `sched_yield`. Threads use the process's single executor, and only
the creator can join a worker. Blocking calls let peers run. Mutexes hand ownership
to queued lockers in arrival order.

The SDK defaults condition variables to `CLOCK_MONOTONIC`, including
`PTHREAD_COND_INITIALIZER`, so default deadlines work before UTC is synchronized.
Set `CLOCK_REALTIME` explicitly with `pthread_condattr_setclock` for UTC deadlines.
Workers can return a result or call `pthread_exit`, both of which run their TLS
destructors. Main ends the process by returning or calling `exit`. Calling
`pthread_exit` from main aborts. Worker cancellation is outside this subset.

The following stubs report `ENOSYS`:

| Calls | Pending support |
|---|---|
| `posix_spawn`, `waitpid` | POSIX wrappers for the process ABI (core#762) |
| `socket`, `connect`, `bind`, `listen`, `accept`, `send*`, `recv*`, `shutdown` | the network capability in the startup structure (core#717); DNS (`getaddrinfo`) is not declared |

Never provided: `fork`, asynchronous signals, general `mmap`.

## Licences

picolibc is BSD-licensed; `COPYING.picolibc` (the licence of the exact release in
`sdk.json`) ships in every archive and must stay with redistributed binaries
that link `libc.a`. Core's own sources (crt0, stubs, the POSIX layer, the example)
are Apache-2.0 (`LICENSE-xila.md`). On Cortex-M and RISC-V the link adds the
GNU toolchain's `libgcc.a` (GPL with the GCC Runtime Library Exception); the SDK
does not redistribute it.

## Building the SDK

```sh
mise run sdk:build      # target/sdk/xila-sdk-<abi>-<target>.tar.zst
mise run sdk:verify     # build the example, assert the ELF contract, report sizes
XILA_SDK_TARGET=thumbv7em-none-eabihf mise run sdk:build ::: sdk:verify
XILA_SDK_TARGET=riscv32imac-unknown-none-elf mise run sdk:build ::: sdk:verify
```

`tools/sdk/build.py` downloads the pinned picolibc release (`picolibc_version` and
`picolibc_sha256` in `mise.toml`; a mismatching archive is never unpacked),
configures it with meson and ninja through `uv run`, compiles the SDK sources and
packs a deterministic archive. `tools/sdk/check_elf.py` asserts that an image is
`ET_DYN`, has no `PT_INTERP` or `PT_TLS`, carries the `XILA` ABI note, has no
`DT_NEEDED`, `DT_INIT`, `DT_FINI` or non-empty init/fini arrays, uses only the
machine's `RELATIVE` dynamic relocations, and (on RISC-V) defines no
`__global_pointer$`. On
Xtensa the link will use `-pie --no-dynamic-linker`, never `-static-pie`. The
build needs `clang`, `lld`, `llvm-ar`, `cmake`, `zstd` and, for Cortex-M and RISC-V,
the Arm GNU toolchain or `gcc-riscv64-unknown-elf`; CI installs them through mise
bootstrap packages. The SDK only takes `libgcc.a` from the Arm package, so CI installs
`gcc-arm-none-eabi` without its recommended newlib (about 490 MB).
