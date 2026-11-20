# Xila native SDK

Compile C against picolibc and a POSIX layer, link a static PIE, and run it on
Xila without rebuilding the firmware. Core's CI builds one archive per target
and uploads it as a workflow artifact:

    xila-sdk-<abi>-<target>.tar.zst      for example xila-sdk-0.1-thumbv7em-none-eabihf.tar.zst

Targets today: `x86_64-unknown-linux-gnu` (the host Platforms runs on Linux) and
`thumbv7em-none-eabihf` (Cortex-M4F). RISC-V (`riscv32imac`) and Xtensa
(`xtensa-esp32s3`) are additions to the build tables, not redesigns.

## Layout

```text
xila-sdk-0.1-<target>/
  sdk.json                  target, ABI, picolibc pin and options, compiler
  COPYING.picolibc          picolibc's licence (BSD); LICENSE-xila.md is Core's (Apache-2.0)
  include/                  picolibc headers, xila/application.h, xila/stubs.h,
                            xila/posix.h, sys/random.h, sys/socket.h, pthread.h
  lib/
    crt0.o                  _xila_start: checks the startup, installs the API table
    libxila.a               indirect-call stubs for every XilaApi slot
    libxila_posix.a         the POSIX layer; also defines xila_main, which calls main
    libc.a, libm.a          picolibc (libm.a is empty: the math library is inside libc.a)
    xila-static-pie.ld      the loader's linker script
  cmake/xila-toolchain.cmake
  share/xila/example/hello  the example below
```

## Building an application

Needs `clang`, `lld` and CMake. On Cortex-M the 64-bit and soft-double helpers
come from a compiler runtime: install the Arm GNU toolchain (`gcc-arm-none-eabi`)
or pass `-DXILA_BUILTINS_LIBRARY=<libgcc.a>`.

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

Copy `hello.elf` to a filesystem the platform's native format reads (Core's
`NativeFormat`, the `native-dev` feature) and execute it. `main(int argc, char **argv)`
receives the arguments; `argv[0]` is empty until the host passes the program name.
Returning from `main` and `exit` end the program after the `atexit` handlers have run and
every stream is flushed; `_exit` ends it at once, without handlers and without flushing,
as POSIX requires (the descriptors the program left open are still closed). A stream
written and not flushed before `_exit` loses its data.

### The example

`share/xila/example/hello/hello.c` exercises `printf`/`snprintf`, `malloc`, files
(`open`, `read`, `write`, `lseek`, `pread`, `fstat`, `rename`, `unlink`), directories
(`mkdir`, `chdir`, `getcwd`, `opendir`, `readdir`), `fopen`/`fgets`/`fprintf`,
clocks, `nanosleep` and `getentropy`. Core's test builds it with this toolchain file
and runs it through the VFS, the executable dispatch table, the native loader and
the ABI table (`libraries/executable/tests/sdk.rs`).

## How it maps onto the native ABI

* **No ambient authority.** Core's ABI takes a directory capability and a pointer plus
  a length for every path. The layer keeps an `int` file-descriptor table and a
  current-directory string, normalizes paths lexically (`.` and `..` never reach Core;
  `..` does not look through symbolic links), and resolves absolute paths against the
  *root capability*, which today is `XilaStartup.home`. The host grants it with
  `NativeFormat::with_home(path, rights)` until the per-launch home of the process ABI
  (core#759) replaces it. Without a home, path calls fail with `EACCES`.
  When the ABI adds `root` and `cwd` handles (core#717) the layer prefers them.
* **Descriptors.** Slots 0-2 are the startup streams; `close(0)` detaches without
  closing host-owned objects. The table is on the process heap: eight slots (8 bytes
  each on 32-bit targets) at first, doubling to at most 256.
* **Errors.** `errno` comes from one table keyed on `XilaStatus` wire values
  (`posix/status.c`). Unknown statuses become `EIO`.
* **Heap.** `malloc`, `calloc`, `realloc` and friends are the process heap
  (`xila_heap_*`); picolibc's own allocator is not built. Alignments above
  `XILA_HEAP_ALIGNMENT` fail.
* **stdio.** `stdin`, `stdout` and `stderr` are descriptors 0-2 through picolibc's
  buffered POSIX streams, each in its own object, so a program that never reads
  `stdin` carries neither its `FILE` nor its buffer. `stdout` is line buffered (128
  bytes by default; override with `-DXILA_STDOUT_BUFFER=n` when building the SDK);
  `stderr` is unbuffered, so each write reaches the host when it is made. `printf` and `scanf` are integer-only by default;
  configure with `-DXILA_PRINTF_FORMAT=double` (or `float`, `long-long`, `minimal`)
  to link picolibc's other variants. Floating point costs several KB of flash.
* **errno and libc state.** picolibc is built with `-Dthread-local-storage=false
  -Derrno-function=__xila_errno_location`. There is one coroutine per application
  today, so `errno` is process-wide; `__xila_errno_location` is the only function to
  change when the ABI exposes coroutine-local storage (core#718). `strtok`, `rand`
  and the other libc statics stay process-wide until then.
* **Exit.** `exit` runs the `atexit`, `on_exit` and `__cxa_atexit` handlers in reverse
  order, then `fflush(NULL)`, which flushes every `FILE` picolibc tracked
  (`stdio-exit-flush`) and `stdout` and `stderr`, then calls `_exit`. `_exit` jumps back
  to `xila_main`, which closes the descriptors and returns the status. picolibc's own
  exit-time machinery is replaced because it cannot run here: its handler table runs from
  a `.fini_array` entry and its stdio flush registers itself from a constructor, and
  that flush also forgot each stream it flushed, so a program that called `fflush(NULL)`
  lost its later writes at exit. The build removes those archive members from `libc.a`
  (`DROPPED_MEMBERS` in `tools/sdk/build.py`) and the layer defines the replacements
  (`posix/onexit.c`, `posix/exit_flush.c`). The build also scans every member of every
  SDK archive and fails on any `.init_array`, `.fini_array`, `.preinit_array`, `.ctors`
  or `.dtors` section, so no future picolibc update can hand an application a
  constructor.
* **Directories.** Reading a directory stream gives `d_ino` a stable nonzero value
  derived from the directory's path and the entry's name (the iterator reports no
  inode; it is not `st_ino`). `open` of `/` or `.` read-only gives a directory
  descriptor (`fstat`, `openat` base); with write access, or `O_DIRECTORY` with write
  access, it fails with `EISDIR`.
* **Path escapes.** `openat` with a directory descriptor cannot leave that directory;
  the error is the errno of Core's `PATH_ESCAPES_ROOT` status (`EACCES`, as the
  `io_error` catalog projects it).

## POSIX subset

Provided: `open`, `openat`, `creat`, `close`, `read`, `write`, `pread`, `pwrite`
(emulated by seeking and restoring the cursor: not atomic against other users of the
descriptor), `lseek`, `fsync`, `fdatasync`, `ftruncate`, `fstat`, `stat`, `lstat`,
`fstatat`, `mkdir`, `mkdirat`, `unlink`, `unlinkat`, `rmdir`, `rename`, `renameat`,
`opendir`, `readdir`, `rewinddir`, `closedir` (no `.` or `..` entries), `getcwd`,
`chdir`, `isatty` (always false), `clock_gettime`, `clock_getres` (`CLOCK_REALTIME`,
`CLOCK_MONOTONIC`), `nanosleep`, `sleep`, `usleep` (not interruptible: there are no
signals), `time`, `gettimeofday`, `getentropy`, `getrandom`, the `malloc` family, C
`stdio`, `exit`, `_exit` and `abort`. Mode bits passed to `open` and `mkdir` are ignored.

Return `ENOSYS`, until the ABI slots they need exist:

| Calls | Waiting for |
|---|---|
| `posix_spawn`, `waitpid`, environment, user and process ids | core#717 (process and environment capabilities) |
| `pthread_create`, `pthread_join`, `pthread_detach` (no mutexes or condition variables are declared) | core#718 (threads, coroutine TLS, synchronization) |
| `poll`, `pipe`, `dup`, `dup2` | core#719 (readiness, pipes, descriptor operations) |
| `socket`, `connect`, `bind`, `listen`, `accept`, `send*`, `recv*`, `shutdown` | the network capability in the startup structure (core#717); DNS (`getaddrinfo`) is not declared |

Never provided: `fork`, asynchronous signals, general `mmap`.

## Licences

picolibc is BSD-licensed; `COPYING.picolibc` (the licence of the exact release in
`sdk.json`) ships in every archive and must stay with redistributed binaries
that link `libc.a`. Core's own sources (crt0, stubs, the POSIX layer, the example)
are Apache-2.0 (`LICENSE-xila.md`). On Cortex-M the link adds the Arm GNU
toolchain's `libgcc.a` (GPL with the GCC Runtime Library Exception); the SDK does
not redistribute it.

## Building the SDK

```sh
mise run sdk:build      # target/sdk/xila-sdk-<abi>-<target>.tar.zst
mise run sdk:verify     # build the example, assert the ELF contract, report sizes
XILA_SDK_TARGET=thumbv7em-none-eabihf mise run sdk:build ::: sdk:verify
```

`tools/sdk/build.py` downloads the pinned picolibc release (`picolibc_version` and
`picolibc_sha256` in `mise.toml`; a mismatching archive is never unpacked),
configures it with meson and ninja through `uv run`, compiles the SDK sources and
packs a deterministic archive. `tools/sdk/check_elf.py` asserts that an image is
`ET_DYN`, has no `PT_INTERP` or `PT_TLS`, carries the `XILA` ABI note, has no
`DT_NEEDED`, `DT_INIT`, `DT_FINI` or non-empty init/fini arrays, uses only the
machine's `RELATIVE` dynamic relocations, and contains no `R_RISCV_RELAX`. On
Xtensa the link will use `-pie --no-dynamic-linker`, never `-static-pie`. The
build needs `clang`, `lld`, `llvm-ar`, `cmake`, `zstd` and, for Cortex-M, the Arm
GNU toolchain; CI installs them through mise bootstrap packages.
