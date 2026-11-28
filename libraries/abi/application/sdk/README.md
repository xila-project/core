# Xila native SDK

Compile C against picolibc and a POSIX layer, or Rust against the raw
`xila-sys` bindings. Link a static PIE and run it on Xila without rebuilding
the firmware.

To get the SDK for a Xila release:

1. Open the `vX.Y.Z` release that matches your Xila installation on the
   [Platforms releases page](https://github.com/xila-project/platforms/releases).
2. Download `SHA256SUMS` and `xila-sdk-<xila_version>-<target>.tar.zst` for
   your target. `<xila_version>` is the major and minor version of the release.
   For example, Xila `v0.1.0` uses `xila-sdk-0.1-thumbv7em-none-eabihf.tar.zst`
   for Cortex-M4F. The archive name does not show the patch version. Record
   the release tag next to the archive.
3. From the download directory, run
   `sha256sum --check --ignore-missing SHA256SUMS` (GNU coreutils). Make sure
   that the output has an `OK` line for your archive: `--ignore-missing` skips
   an absent file, and the command still succeeds. The check finds a damaged
   download. It does not prove who made the release, because `SHA256SUMS`
   comes from the same release as the archive.
4. Extract the archive. Each archive includes `COPYING.picolibc` and
   `LICENSE-xila.md`.

Core CI also uploads each build as a workflow artifact named
`xila-sdk-<target>`. Workflow artifacts need a GitHub sign-in and expire after
the repository's retention period (90 days by default), so use the release
assets for application builds.

| Target | Class | Float ABI | Compiler runtime |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | the host Platforms runs on Linux, x86-64 | none | none needed |
| `aarch64-unknown-linux-gnu` | the host Platforms runs on Linux, AArch64 | none | none needed |
| `thumbv7em-none-eabihf` | Cortex-M4F | hard | Arm GNU toolchain's `libgcc.a` |
| `riscv32imac-unknown-none-elf` | ESP32-C6 class RV32IMAC | soft (`ilp32`) | bare-metal RISC-V GCC's `libgcc.a`, or Espressif `riscv32-esp-elf-gcc`'s |
| `riscv32imafc-unknown-none-elf` | ESP32-P4 class RV32IMAFC | single (`ilp32f`) | bare-metal RISC-V GCC's `libgcc.a`, or Espressif `riscv32-esp-elf-gcc`'s |
| `xtensa-esp32s3-elf` | ESP32-S3 | toolchain default | Espressif `xtensa-esp32s3-elf-gcc`'s `libgcc.a` |

The RISC-V images do no gp relaxation: the link defines no `__global_pointer$`
(and never passes `--relax-gp`), and `check_elf.py` rejects an image that defines
it, because the image's gp is not the firmware's. Call relaxation (pc-relative
`jal`) stays on: it is valid in a position-independent executable. They cannot
run on the Linux host loader, so CI validates them structurally with
`check_elf.py`, like Cortex-M.

## Layout

```text
xila-sdk-0.1-<target>/
  sdk.json                  target, Xila version (`abi`), picolibc pin and options, compiler
  COPYING.picolibc          picolibc's licence (BSD); LICENSE-xila.md is Core's (Apache-2.0)
  include/                  picolibc headers, xila/application.h, xila/stubs.h,
                            xila/graphics.h, xila/posix.h, sys/random.h, sys/socket.h,
                            netinet/in.h, netinet/tcp.h, arpa/inet.h, netdb.h,
                            pthread.h, semaphore.h
    posix/stdlib.h          wrapper for the SDK's getenv declaration
  lib/
    crt0.o                  _xila_start: checks the startup, installs the API table
    crt0_linked.o           xila_linked_start, for a program linked into the firmware
                            (link-archive): same checks, no ABI note, no _xila_start
    libxila.a               indirect-call stubs for every XilaApi slot, and the graphics
                            functions of xila/graphics.h
    libxila_posix.a         the POSIX layer; also defines xila_main, which calls main
    libc.a, libm.a          picolibc (libm.a is empty: the math library is inside libc.a)
    xila-static-pie.ld      the loader's linker script
  rust/xila-sys/             standalone Rust crate
    Cargo.toml, Cargo.lock
    src/                    raw bindings, optional entry runtime and graphics module
    build.rs                supplies the guest linker arguments with the link feature
    xila-static-pie.ld      copy of the loader's linker script used by build.rs
  rust/xila-version/         bundled Xila version crate
    Cargo.toml, src/lib.rs
  cmake/xila-toolchain.cmake
  share/xila/example/hello  the example below
  share/xila/example/hexdump  the hexdump utility (hex + ASCII dump and its -r reverse)
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

`share/xila/example/hexdump/hexdump.c` is a small POSIX C11 utility:
`hexdump [-n length] [-s skip] [-c columns] [-r] [-h] [file ...]` prints the
canonical hex + ASCII dump of each file (or of stdin) and `-r` reverses it back
to raw bytes. It uses only `getopt`, stdio, `strerror(errno)` and the heap, keeps
no mutable statics of its own and fits a few hundred bytes of stack, so it also
shows the footprint a real application has on a microcontroller. `sdk:verify`
builds its `hexdump.elf` and its `link-archive` archive (`libhexdump.a`, entry
`xila_linked_hexdump`) for every target and reports both sizes, and Core's test
runs the same suite against the loaded ELF and the linked archive
(`libraries/executable/tests/sdk.rs`, `test:native-loader`).

## Linked C programs: `link-archive`

A C program can also be linked into the firmware itself, instead of being loaded as an
image. The SDK's `link-archive` command turns one such program into a static archive
whose only global defined symbol is its entry:

```sh
# Compile the program with -fPIE, like every SDK object (crt0_linked.o included), and
# the SDK's headers: position-independent code links into both the firmware's image and
# PIE host binaries (Linux tests, Rust test binaries), which non-PIC code cannot.
clang --target=x86_64-unknown-none-elf -c app.c -o app.o \
  -std=gnu11 -Os -fPIE -ffunction-sections -fdata-sections -fno-stack-protector \
  -fno-unwind-tables -fno-asynchronous-unwind-tables -D_GNU_SOURCE -nostdinc \
  -isystem <sdk>/include/posix -isystem <sdk>/include -isystem <clang resource dir>/include
# Localize it: one archive, one global symbol.
python tools/sdk/build.py link-archive \
  --sdk <sdk> --entry my_app --output libmy_app.a app.o
```

`link-archive` runs `ld.lld -r` over `<sdk>/lib/crt0_linked.o`, your objects or archives
and `--start-group <sdk>/lib/libxila_posix.a <sdk>/lib/libxila.a <sdk>/lib/libc.a
[compiler runtime] --end-group` (the compiler runtime, Arm's or RISC-V's `libgcc.a`,
joins the group when the SDK's target has one and its cross compiler is installed;
`--builtins <archive>` adds one explicitly), producing one relocatable object with
`xila_linked_start` and your `main` pulled in. It then inspects the undefined symbols
(`llvm-nm --undefined-only`): every weak undefined one is resolved to the absolute 0, so it
reads as null in the firmware instead of binding to a host symbol (glibc's `stderr` in a
Linux PIE test binary, for example), and any strong undefined symbol is an error unless
the firmware provides it (`__xila_current_startup`, or `--allow-undefined <symbol>`).
It then renames `xila_linked_start` to `--entry` and localizes every other symbol
(`llvm-objcopy --redefine-sym`, then `--keep-global-symbol=<entry>`), and packs the
result with `llvm-ar rcs`. `--entry` must be a C identifier
(`^[A-Za-z_][A-Za-z0-9_]*$`).

Every libc, POSIX, crt0 and application global or static of the program becomes local,
so `main`, `malloc`, `printf`, `exit`, `stdout`, `xila_process`, `xila_sdk_api`, ...
never collide with the host firmware's symbols, with bare-metal `compiler_builtins`,
or with another linked program's. Two archives built this way link into one firmware
without a single colliding symbol, and `nm <lib.a>` shows the entry as the only global
`T` symbol and no undefined one except the allowed ones.

`crt0_linked.o` is `crt0.c` compiled with `-DXILA_SDK_LINKED` and the same `-fPIE` as every
other SDK object: it emits no `.note.xila.abi` and no `_xila_start`, and defines
`int32_t xila_linked_start(const XilaStartup *startup)` with the same
startup/API/version compatibility check as `_xila_start`, installing `xila_sdk_api` and
`xila_sdk_process` around the call to `xila_main` (the POSIX layer, which calls your
`main`) and restoring them to the fallback table and NULL on return.

### Relaunching a linked program

`xila_linked_start` resets the SDK and POSIX runtime state on every launch: the descriptor
table, the standard streams and their locks and buffers, the stdio file list, the
environment tables and cache, the exit-handler table, the spawned-child list, the
once-call list, the key-destructor table, the global libc lock, the `strtok` state and
picolibc's `getopt` globals (`optind`, `opterr`, `optopt`, `optarg`) all go back to their
pristine initial values before `xila_main` runs. This holds whichever way the previous run
ended: a normal return or exit already cleaned up, but a cancellation (the run future
dropped while the program was suspended) runs no cleanup, and the host frees the process
heap with the abandoned coroutine, so the reset is what keeps the next run from touching
freed memory. Each module provides its reset hook, and a weak no-op default in
`startup.c`/`fd.c` covers the modules that are not linked, so they cost no code.

Known picolibc limits: file-static heap state inside picolibc's `hcreate`/`hsearch` hash
table and `getpwent`/`getgrent` stream iterators is not exposed for reset, so avoid those
functions in relaunched linked programs.

Application-owned C `static` variables are different: they live in the program's own
`.data`/`.bss`, which persists across launches of the same program. Initialize them at the
top of `main` (or use the `exclusive` form and accept one instance at a time) if a relaunch
must not see the previous run's values.

Link the archive into the firmware and run it with `xila-executable`'s linked-program
support, in the `exclusive` form, because two concurrent launches of the *same* program
share that program's `.data`/`.bss`:

```rust
executable::linked_executable!(unsafe pub MyApp => my_app, exclusive, stack_size = 64 * 1024);
```

`mount_executables!` mounts the declared type like any other executable. A second
concurrent launch of an exclusive program fails immediately with `BUSY_FAILURE` (16,
`EBUSY`) without touching the entry or its globals; the slot is released when the
program returns or exits, and also when its run is cancelled while suspended.
`executable::run_linked_exclusive` and `executable::ExclusiveSlot` are the underlying
API. Use the plain (non-exclusive) form only for programs whose globals are safe to
share between concurrent instances, such as a Rust program that keeps its state in the
host's per-process startup block. Two *different* localized programs can run
concurrently in the plain form: their localized globals are private to each.

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
The entry macro emits `_xila_start` and the ELF ABI note, checks both sizes and that the
table's Xila version is compatible with the one the crate was generated for, then calls your `unsafe fn main(*const xila_sys::XilaStartup) -> i32`.
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
guest builds that enable `link` in separate Cargo invocations. To keep host tests
off `link`, put the host-tested code in a library that depends on `runtime` only,
or enable `xila-sys/link` through a feature of the application. A bare-metal build
with `runtime` but not `link` does not link (`cannot find entry symbol _start`), so
`xila-sys` warns that it needs `link`.

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

## Graphics

Native programs draw with the same API as Wasm applications. `xila/graphics.h`
declares one function per supported call, with the name, arguments and result of
the Wasm guest API (`xila_graphics_label_create`, `xila_graphics_label_set_text`,
`xila_graphics_window_pop_pointer_event`, ...), and `libxila.a` defines them.
`xila_sys::graphics` is the Rust counterpart with the guest API's function names,
types and `Result`; a Rust application written against the guest API builds
natively with little change. Both are generated from the declaration that
generates the Wasm guest bindings and Core's executor, so the three cannot
disagree and no call is written by hand. `xila_graphics_object_create(parent, &object)`
in C and `object_create(parent)` in Rust both produce the new object, which is a
32-bit handle held in a pointer-sized value, never an address.

```c
#include <xila/graphics.h>

XilaGraphicsObject *window, *label;
if (xila_graphics_window_create(&window) != 0 ||
    xila_graphics_label_create(window, &label) != 0)
    return 1;
xila_graphics_label_set_text(label, "Hello");
```

Every function returns 0, or the non-zero `XilaStatus` the host reported:
`XILA_STATUS_INVALID_ARGUMENT` for a wrong handle, argument or value (including a
string with no terminator within the host's limit), `XILA_STATUS_OUT_OF_MEMORY` when a
buffer or a per-call limit is exceeded, `XILA_STATUS_UNAVAILABLE` when the system has
no display, and `XILA_STATUS_UNSUPPORTED` when the firmware was built without
graphics. These are `XilaStatus` values: a Wasm guest receives the executor's codes 1
to 8 instead, so an error-code check is not portable between the two. A
result that is a pointer-sized word (a handle, or the target of an event) is
written as a whole `size_t`; the others are 32 bits. Strings are NUL-terminated
and capped by the host: a longer one, or an unterminated one, is refused after
the host has read at most that many bytes.

Events come from the window's queue: `xila_graphics_window_pop_event` and
`xila_graphics_window_pop_pointer_event` report an event code
(`XILA_GRAPHICS_EVENT_CODE_CLICKED`, ...) and the handle of its target, or code 0
for no event. Poll them from your main loop and sleep (`xila_time_sleep`) when the
queue is empty, so the other tasks run.

How it works, and why raw `lv_*` functions are not exposed:

* One `XilaApi` slot, `graphics_call(process, id, words, count, result)`, carries
  every call. The generated functions pack their arguments into words, so adding a
  call to Core adds no slot.
* Core's graphics executor runs the call under the graphics lock and keeps a
  per-process table of the objects the process created. A handle names one of them;
  an object that is not the process's is refused. When the process ends (`main`
  returns, `exit` or `xila_process_exit`), the host deletes every object it still
  owns, windows included, so nothing needs cleaning up by hand. The table lives in the
  process's host-owned state, not on its stack, so the same cleanup covers a task
  cancelled while suspended, and with it goes the task identity that file pickers act
  with.
* Raw LVGL functions would bypass both: they would not take the lock, and the objects
  they create would belong to nobody, so an exited or cancelled process would leak them
  and could corrupt the display by deleting an object another process owns. An `lv_*`
  compatibility layer can be built on top of this API later; it cannot be built
  beneath it.
* The graphics lock is awaited by suspending the process, like a file read, so a
  program that draws never blocks the other tasks. The first graphics call of a
  process allocates its table; a process that never draws pays one pointer.
* Native code is trusted, so pointers are read as they are; only strings and buffers
  are bounded by the executor's limits.

Core's `sdk/test/graphics` program builds a window with a button, a label, a checkbox
and a slider, reads properties back, packs a color, checks the refused cases and polls
for a click; the Linux loader test clicks it and checks the host deleted everything,
whether the program returned or called `exit`.

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
  empty (`array[0]` is NULL) and joins a small pool that later tables reuse.
  Pooled arrays are never freed, so memory does not grow with the number of
  adoptions: it is bounded by the largest table, and at most three arrays per
  size class (the current table, the retained one and the one being built).
  Capacities are powers of two from four slots. On a 32-bit target, the three
  128-slot arrays a 64-entry table needs cost about 1.5 KB at worst, and the
  smaller size classes reached on the way add at most as much again. Restoring a table saved before
  the most recent adoption is therefore safe but not supported: the next
  accessor adopts whatever that array holds, which is empty unless a later table
  reused it, in which case it is that table's current contents. A mutation of
  the current table can invalidate saved pointers to its strings, and can change
  its entries in place or move them to another array. When `setenv` adds a name
  and the array has no spare slot, the entries move to a new array and the old
  one reads as empty. Restoring a table saved before that `setenv` then adopts
  that empty array: it empties the environment, the host's included. Removing the
  last entry leaves the current array in place with a NULL first slot.

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
  (its thread identifier and the pointer to its process group) lives in its record on the
  host's heap, not on the worker's coroutine stack, so the same holds for an overflow in a
  worker. An image
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
timed waits, read-write locks, and `sched_yield`. Threads use the process's single executor, and only
the creator can join a worker. Blocking calls let peers run. Mutexes hand ownership
to queued lockers in arrival order.

The SDK defaults condition variables to `CLOCK_MONOTONIC`, including
`PTHREAD_COND_INITIALIZER`, so default deadlines work before UTC is synchronized.
Set `CLOCK_REALTIME` explicitly with `pthread_condattr_setclock` for UTC deadlines.
Workers can return a result or call `pthread_exit`, both of which run their TLS
destructors. Main ends the process by returning or calling `exit`. Calling
`pthread_exit` from main aborts at once, without running destructors. A `pthread_once`
routine that leaves its thread through `pthread_exit` leaves the control as if it had
never been called, so the next caller runs the routine. Worker cancellation is outside
this subset.

Read-write locks (`pthread_rwlock_*`): plain, try, timed (`CLOCK_REALTIME`) and clock
(`CLOCK_MONOTONIC` or `CLOCK_REALTIME`) read and write locks, `PTHREAD_RWLOCK_INITIALIZER`
and attributes. Readers are preferred, as in glibc and musl: a reader enters whenever no
writer holds the lock, so a thread can take the read lock again while it holds it, and a
steady stream of readers can keep a writer waiting. A thread that holds the write lock
gets `EBUSY` from a try call and `EDEADLK` from a blocking call. A thread that only
holds a read lock cannot upgrade it. A try write lock returns `EBUSY`, a timed write
lock can time out, and an untimed blocking write lock waits indefinitely. Processes
share no memory, so `PTHREAD_PROCESS_SHARED` fails with `ENOTSUP`.

Unnamed semaphores (`<semaphore.h>`): `sem_init`, `sem_destroy`, `sem_post`, `sem_wait`,
`sem_trywait`, `sem_timedwait` (a `CLOCK_REALTIME` deadline), `sem_clockwait`
(`CLOCK_MONOTONIC` or `CLOCK_REALTIME`) and `sem_getvalue`. A semaphore is an ABI mutex
and condition variable around its count, so blocked waiters let peers run and get their
units in arrival order. `SEM_VALUE_MAX` is `INT_MAX`, and a post beyond it fails with
`EOVERFLOW`. A deadline is checked only when the call would block. Processes share no
memory, so `sem_init` with a nonzero `pshared` fails with `ENOSYS`, and so do the named
semaphore calls `sem_open`, `sem_close` and `sem_unlink`.

### Processes: `posix_spawn` and `waitpid`

`posix_spawn` and `posix_spawnp` start a child through `xila_process_spawn`, and the
returned process identifier is the one the child reports with `getpid`
(`xila_process_child_id`).

- `posix_spawnp` searches `PATH` and has no default search path: without `PATH`, only a
  name containing a slash runs. An empty entry is the current directory.
- A child gets exactly three descriptors, its standard streams. File actions edit that
  view in order: `addopen`, `addclose` and `adddup2` work on descriptors 0 to 2.
  Closing a descriptor above 2 does nothing, because the child inherits none. Opening
  onto or duplicating onto a descriptor above 2 fails with `ENOTSUP`. A child left
  without one of the three streams fails with `EBADF`.
- A null environment array makes the child inherit the parent's environment. A non-null
  array, empty or not, is the child's whole environment.
- `POSIX_SPAWN_RESETIDS`, `POSIX_SPAWN_SETSIGMASK` and `POSIX_SPAWN_SETSIGDEF` are
  accepted and have no effect. `POSIX_SPAWN_SETPGROUP`, `POSIX_SPAWN_SETSCHEDULER` and
  `POSIX_SPAWN_SETSCHEDPARAM` fail the spawn with `ENOTSUP`. The scheduling attribute
  calls are not defined.
- `waitpid` with a positive `pid` waits for that child. `-1`, `0` and `-getpid()` wait
  for any child: with one child it waits for it directly, otherwise it polls with sleeps
  from 1 to 16 ms. `WNOHANG` is supported, `WUNTRACED` and `WCONTINUED` are accepted, and
  `wait` is `waitpid(-1, ...)`. A pid that is not an unreaped child gives `ECHILD`.
- The status holds the low 8 bits of the exit status: use `WIFEXITED` and `WEXITSTATUS`.
  There are no signals.

No POSIX call is left as a pending `ENOSYS` stub. Never provided: `fork` (it fails with
`ENOSYS`), asynchronous signals, general `mmap`.

### Sockets and name resolution

`<sys/socket.h>`, `<netinet/in.h>`, `<netinet/tcp.h>`, `<arpa/inet.h>` and `<netdb.h>`
provide `socket`, `bind`, `listen`, `accept`, `connect`, `send`, `recv`, `sendto`,
`recvfrom`, `shutdown`, `getsockname`, `getpeername`, `setsockopt`, `getsockopt`,
`getaddrinfo`, `freeaddrinfo`, `gai_strerror`, `inet_pton`, `inet_ntop` and
`inet_addr`. `read`, `write`, `close`, `dup`, `dup2`, `fcntl`, `poll` and `fstat`
(`S_IFSOCK`) work on socket descriptors. The calls use the network capability in
`XilaStartup.network` (see the ABI README). **A program has no network unless its
launch grants it**, with separate connect, listen and resolve rights. Without a
grant, `socket` fails with `EACCES`, and `getaddrinfo` can only answer numeric
hosts and `localhost`: a lookup returns `EAI_SYSTEM` with `errno` set to `EACCES`.
With some rights only, the call that needs a missing one fails with `EACCES`:
`listen`, or `bind` of a datagram socket, without listen; `connect` or a send
without connect; a lookup without resolve. A child gets at most its parent's
rights (`XilaSpawn.flags`).

A socket is an `AF_INET` or `AF_INET6` stream (TCP) or datagram (UDP) socket.
`socket` accepts `SOCK_NONBLOCK` and `SOCK_CLOEXEC` in its type. Each socket is a
record of 64 bytes (72 on 64-bit targets) on the process heap, shared by its duplicates: closing the
last descriptor closes the native object, and `O_NONBLOCK` set through `fcntl`
applies to every duplicate. An image that never creates a socket does not link the
socket code.

How the subset maps onto Core's network stack:

* **Ports.** Core assigns no port to a bind or a listener, so `bind` with port zero
  fails with `EADDRNOTAVAIL` and `listen` on an unbound socket with `EDESTADDRREQ`.
  An outgoing connection gets its local port from Core. An unbound datagram socket
  is bound, on its first send, to the unspecified address and a random port from
  49152 to 65535. Core does not report a port that another socket already uses.
* **Stream sockets.** Core binds and listens in one step, so `bind` records the
  address and `listen` checks it: an address that is not local fails there. A
  listener holds one pending connection and `accept` replaces it, so the backlog
  is ignored (`SOMAXCONN` is 1). `connect` ignores an address given to `bind`.
  `connect` always blocks, even on a nonblocking socket. `shutdown(SHUT_WR)` sends
  the end of the stream. A later `send` fails with `EPIPE`. There are no signals,
  so `SIGPIPE` is never raised. `shutdown(SHUT_RD)` makes `recv` return zero.
  Sends can be partial. Core's buffers are 1024 bytes in each direction.
* **Datagram sockets.** `connect` sets or, with `AF_UNSPEC`, clears the default
  destination, and the socket then receives only from that peer. A datagram larger
  than the buffer is truncated and its remainder dropped, as in POSIX. Core keeps
  one datagram queued in each direction.
* **Blocking.** Blocking calls suspend the calling thread and let peers run.
  `MSG_DONTWAIT` and `O_NONBLOCK` make `accept`, `send` and `recv` fail with
  `EAGAIN` when the socket is not ready. Only `MSG_DONTWAIT` and `MSG_NOSIGNAL`
  are accepted as flags. Others, `MSG_PEEK` included, fail with `EOPNOTSUPP`.
  `poll` reports a socket that is neither bound nor connected as never ready.
* **Addresses.** A socket keeps its family, as with `IPV6_V6ONLY`: an `AF_INET6`
  socket neither reaches nor hears IPv4 peers, and a connect to an IPv4-mapped
  address (`::ffff:a.b.c.d`) fails with `ENETUNREACH`. The unspecified address
  listens on every local address of the socket's family. A connection of the
  other family to a wildcard listener completes its handshake; the peer's first
  read or write then fails with `ECONNRESET` once the listener is polled or
  accepted. A send to the other
  family fails with `EAFNOSUPPORT`. Peer addresses are always reported in the
  socket's family. An IPv4 and an IPv6 wildcard bind of the same port do not
  coexist (see the ABI README). IPv6 scopes are not supported.
* **Options.** `SO_REUSEADDR`, `SO_KEEPALIVE` and `TCP_NODELAY` are accepted and
  have no effect. `getsockopt` also reports `SO_TYPE`, and `SO_ERROR` is always
  zero. Other options fail with `ENOPROTOOPT`.
* **Names.** `getaddrinfo` takes a decimal port as its service: there is no
  services database, so a name fails with `EAI_SERVICE`. There is no hosts file
  either. `localhost` and a null node are answered locally, and numeric hosts are
  parsed. Other names go to Core's resolver, IPv4 first for `AF_UNSPEC`. The
  canonical name is the node as given. `AI_V4MAPPED`, `AI_ALL` and
  `AI_ADDRCONFIG` have no effect: sockets keep their family, so an IPv4-mapped
  result would be unreachable. Without a socket type, each address gets a
  stream and a datagram result. `inet_pton` and `inet_addr` take only the
  dotted-decimal form with four parts and no leading zeros.

## Licences

picolibc is BSD-licensed; `COPYING.picolibc` (the licence of the exact release in
`sdk.json`) ships in every archive and must stay with redistributed binaries
that link `libc.a`. Core's own sources (crt0, stubs, the POSIX layer, the example)
are Apache-2.0 (`LICENSE-xila.md`). On Cortex-M and RISC-V the link adds the
GNU toolchain's `libgcc.a` (GPL with the GCC Runtime Library Exception); the SDK
does not redistribute it.

## Building the SDK

```sh
mise run sdk:build      # target/sdk/xila-sdk-<xila_version>-<target>.tar.zst
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
bootstrap packages. The SDK only takes `libgcc.a` from the Arm package, but apt installs the
recommended newlib packages with `gcc-arm-none-eabi`: on a GitHub runner, the Arm part is then about a
607 MB download, instead of about 70.6 MB (about 568 MB on disk) for the two packages without them.
`mise run ci:packages-sdk-arm` therefore skips them on GitHub Actions runners only: when
`GITHUB_ACTIONS=true` it writes, after installing the other SDK packages,
`/etc/apt/apt.conf.d/90xila-no-install-recommends` (`APT::Install-Recommends "false";`, from
`mise.apt-no-recommends.toml`). That file stays in place and
applies to every later `apt install` on the machine, which is harmless on a throwaway runner. Anywhere
else the task changes no apt configuration and installs the recommended packages; to install only the
two required packages on your own machine, run
`sudo apt-get install --no-install-recommends gcc-arm-none-eabi binutils-arm-none-eabi`.
