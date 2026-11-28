# Native executable loader

The loader installs trusted static PIEs in contiguous executable RAM or PSRAM.
It checks the ELF header against the target, copies `PT_LOAD` segments, zeros
BSS and gaps, and applies relative `DT_REL` and `DT_RELA` relocations. On
Xtensa, a zero explicit RELA addend selects the in-place addend in the data or
instruction literal. The relocation can target an executable segment regardless
of the presence of `DT_TEXTREL`. The loader rejects interpreters, dynamic
dependencies, native ELF TLS, and symbol-based relocations.

An image must be an `ET_DYN` file with a `PT_DYNAMIC` segment, at most 16
program headers, and power-of-two segment alignments up to 64 KiB. Its
`e_flags` must match the target: Arm needs EABI version 5 and the target's
float ABI (hard on `eabihf`), and RISC-V needs the target's float ABI, with
RVC only if the target has the C extension, RVE only for the embedded ABI,
and no TSO. Other targets
require zero flags, except Xtensa, whose flags are not checked.

On RISC-V, the loader also reads `Tag_RISCV_arch` in `.riscv.attributes`.
It rejects malformed attributes and extensions that the firmware target does
not enable. It checks extension names independently of their version numbers.
For example, an image that requires atomics cannot run on `rv32imc`. If the
section is absent, the loader checks only the ELF header and its architecture
flags as before. The Xila firmware baseline includes `zicsr` and `zifencei`.

### Constructors and destructors

The loader accepts preinitialization, initialization and finalization arrays
from the dynamic table or the corresponding ELF sections. When both are
present, they must describe the same array. Each relocated function pointer
must be nonzero and point into an executable segment. A zero entry gives
`Error::InvalidLifecycleEntry`. The loader rejects legacy `DT_INIT` and
`DT_FINI` functions. When removing the section header table, set both
`e_shoff` and `e_shnum` to zero. Inconsistent table metadata gives
`Error::InvalidSectionHeaders`.

The SDK entries install the ABI on the process coroutine, then run
preinitialization and initialization in ascending order. The POSIX entry
also initializes the POSIX state first. A normal return from `main` or a call
to `exit` runs registered exit handlers, then finalization in reverse order,
then flushes standard I/O. A call to `exit` from a constructor runs the whole
finalization array, even if some constructors have not run. With live worker
threads the POSIX entry skips exit handlers and finalization. `_exit`,
abort and cancellation skip finalization.

The direct C shim and Rust entry run finalizers after the application function
returns if no worker threads are live. A direct call to `xila_process_exit`
or a Rust panic in `panic_abort` skips finalization. The SDK entries require
the full `XilaStartup` layout.

A custom `_xila_start` that supports these arrays must emit an empty `PT_NOTE`
record named `XILA\0` of type 2. Without this record, non-empty arrays give
`Error::MissingLifecycleNote`. Check `XilaStartup.size` before reading
`lifecycle`. When present, call its `run` callback with the lifecycle pointer
and `false` after runtime initialization and with `true` on normal exit.
Call each phase at most once.

`link-archive` rejects lifecycle sections in applications linked into the
firmware with `native-linked`. Those sections could run at firmware startup
outside the application's coroutine. Initialize and finalize linked
applications in their own entry point.

## Building an image

C applications normally use the [SDK](../abi/application/sdk/README.md), which wraps
everything below in a CMake toolchain file. To link by hand, use the
[linker script](../abi/application/sdk/rust/xila-sys/xila-static-pie.ld) and define the C entry
`int32_t _xila_start(const XilaStartup *startup)`. The entry receives borrowed
[application ABI capabilities](../abi/application/README.md). Emit a
`PT_NOTE` record named `XILA\0` of type 1 with a four-byte descriptor in ELF
byte order: the Xila version the image was built for, with the major version in
its high 16 bits and the minor version in its low 16 bits. The loader compares
it with the running version (`xila_version::RUNNING`) by the rule of
`xila_version::compatible`, which mirrors `xila_package_manifest::compatible`:
before 1.0 the minor versions must be equal (an image for 0.1 runs on every
0.1.z and is refused on 0.2); from 1.0 the majors must be equal and the running
minor at least the image's (an image for 1.3 runs on 1.3 and 1.9, not on 1.2 or
2.0). Any other image fails with `Error::IncompatibleXilaVersion`, which names
both versions. The [fixture](fixtures/start.c) shows how to take the version
from the C header.

Link with `-fPIE -pie -nostdlib` and
`-Wl,--no-dynamic-linker,-Bsymbolic,-z,max-page-size=16`. On RV32, also pass
`-mno-relax` and `-Wl,--no-relax` to preserve address calculations independent
of the firmware's `gp`. The script places Xtensa literals before instructions.
Adjust `XILA_DATA_ALIGN` where the memory backend requires permission
boundaries larger than 16 bytes. Custom linkers must preserve the relative
virtual addresses of the loadable segments.

The Linux regression test compiles its C ELF separately with
`sh fixtures/build.sh <output>` from this directory. The fixture reads and
writes through `startup->api`, like every native application. Run
`mise run test:native-loader` from the repository root. The 32-bit ELF parser
is tested on the host through stand-in Arm and RISC-V targets.

## Platform registration

For developer execution directly from a trusted filesystem, enable the
`native-dev` feature and add a `NativeFormat::registration()` entry to
`ExecutableContext.formats`. The handler takes an executable-memory backend,
a file size limit, an image size limit, and a coroutine stack size. Core
rejects raw ELF execution by default and refuses native setuid files. The
platform can register its own `*b"\0asm"` handler for Applications Wasm. Existing
mounted executables use their control command. Contexts constructed with
`ExecutableContext::new` start with an empty format table. Add `formats: &[]`
when migrating literal initializers that do not use file-format dispatch.

Callers loading trusted bytes directly can use `LoadedImage::load` and
`LoadedImage::spawn`. These employ the same entry convention and grant only
supplied capabilities. The file handler passes the standard streams and
arguments, a process capability (`XilaStartup.process`, which lets the program
spawn, wait, exit and use its environment: see the ABI README) and, for a
program that a native process spawned, its explicit environment and home
directory. Set `Startup.process` to make a program a process; without it the
program is started like a linked one, with no process capability. A spawned
program's environment replaces the copy its task made of its parent's before
the entry runs. The task future, not the coroutine stack, owns one allocation
that holds the `LoadedImage` handle, the process `XilaHeap` granted through
`startup->heap` and the host state: the `XilaStartup` block, the process
capability and the launch name, arguments and startup streams they point into, and the
registry of the files, directories, listings, children and sockets the program opened.
The image bytes stay in the platform's `ExecutableMemory`, which the handle releases, and
the C heap's blocks are separate allocations the `XilaHeap` tracks: neither is
part of it. On `thumbv7em-none-eabihf`, the default loader features use
512 bytes for the process record. The lifecycle record adds 28 bytes to the
image handle and a 4-byte pointer to the host state. The measured process
allocation grows by 24 bytes after padding. The launch name and argument
strings are allocated separately. The coroutine entry receives only a
pointer to it, so a program that overruns its own frames cannot damage the state
`xila_process_exit` and the SDK's heap and standard error access depend on, and
the host can reach everything the program holds whatever the coroutine was doing.

The allocation is released when the program returns, exits or is cancelled, and in
that order on every path: the coroutine is dropped first, then the state is released,
then it and the allocation are freed. Dropping the task future of `LoadedImage::spawn`
(or the future of `LoadedImage::start`, which runs the program on the calling task)
while the program is suspended cancels it. Stackful discards its frames, frees its
stack and drops the cancellable futures it was suspended on; the loader then closes
every handle the program still held, its startup streams and its home directory,
deletes its graphics objects (and with them the task identity file pickers act with),
frees its arguments and unmaps its image. A program that never started, a spawn the task
manager refused and an allocation that failed release the same things. Nothing is
leaked on any of these paths, and a program that returns without closing its files has
them closed. A program cancelled while a close waits for the network stack's lock loses
that one UDP socket: see the ABI README's *Ownership and cancellation*.

The platform implements `ExecutableMemory`, including cache synchronization
and executable permissions appropriate to its hardware
([Platforms #115](https://github.com/xila-project/platforms/issues/115)).
The trait accepts separate instruction and data aliases for the same allocation
on Xtensa. The loader recognizes Xtensa ELF images and builds its coroutine
backend for Xtensa. Test progress for Xtensa hardware is tracked in
[Core #795](https://github.com/xila-project/core/issues/795).

Image size is the contiguous virtual span including BSS, gaps and alignment
padding. The raw-file handler holds the original file buffer until installation
finishes. Running adds the configurable coroutine stack and 64-byte guard,
the stackful control allocation, and the normal task and startup allocations.

## GDB symbol registration

Enable the `gdb-jit` Cargo feature on `xila` or `xila-executable`. It also
enables `native-dev`. Direct users can enable `gdb-jit` on
`xila-native-loader`. The feature is off by default. The platform must supply
a `critical-section` implementation that excludes other cores when applicable.

Use a firmware ELF that retains `__jit_debug_descriptor` and
`__jit_debug_register_code` and an application ELF that retains `.symtab`.
Load the firmware ELF in GDB and connect to the target through the GDB server.
Set `set breakpoint pending on`, then use `tbreak <function>` before you
launch the application. For a bare-metal target, the server must support
breakpoints at the firmware notification hook and reads of the descriptor,
the entries, and their symbol files. If you attach after a load, GDB can read
the entries for images that are still mapped.

After relocation and memory finalization, the loader builds a compact symbol
file from the original ELF. It copies the section headers, symbol table and
string tables. It sets the runtime addresses of loaded sections and their
symbols, including the instruction alias when the platform has one. It uses
ELF sections without file contents to describe the mapped application. The
symbol file supports function and object names. Source lines and unwind
information are omitted. If the symbol table is absent, the loader cannot use
its format, or the allocation fails, the image still loads.

The image owns the registration and symbol file. The loader removes the entry
and notifies GDB before it frees the symbol file and releases the executable
mapping. Registration and removal use the same critical section. See the
[GDB JIT interface](https://sourceware.org/gdb/current/onlinedocs/gdb.html/JIT-Interface.html).

To load symbols manually on a unified-address target, read the allocation
base with `LoadedImage::base()` and run `add-symbol-file <elf> -o <base>`.
The supplied linker script links at address zero. For separate instruction
and data aliases, pass the runtime section addresses with
`add-symbol-file <elf> <text-address> -s <section> <address> ...`.

### Measurement on `thumbv7em-none-eabihf`

On Rust 1.97.0, a release link that calls `LoadedImage::load` and drops the
result used 3,749 more bytes of flash with the feature enabled. This is the
change in the combined `.text`, `.rodata`, `.ARM.exidx`, and initialized
`.data` sections in a minimal link without link-time optimization. The
descriptor uses 16 bytes of static RAM. The image handle increases from
56 to 60 bytes. Each registration uses one allocation for a 24-byte entry
and the symbol file.

The SDK's Thumb `hello.elf` built with CMake `MinSizeRel` and Clang 22.1.8
has 26 section headers. Its ELF header, section headers, symbol table and
string tables total 18,833 bytes. Registration requests 18,857 bytes of heap
for this example, excluding allocator overhead. This allocation is also the
additional heap used during registration. The loader copies no executable or
data section contents into it.

### Linux batch test

Run `mise run test:native-loader` to build the SDK and verify the descriptor
through return, explicit exit and cancellation. To check the debugger, use
`cargo test --locked -p xila-executable --features gdb-jit --test sdk --no-run`.
Use the test executable path printed by Cargo in this command.

```sh
gdb -nx -batch -x libraries/native_loader/fixtures/gdb-jit.gdb \
  --args <sdk-test-executable> --exact registers_sdk_image_symbols
```
