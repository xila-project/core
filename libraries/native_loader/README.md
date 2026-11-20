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
RVC only if the target has the C extension and no RVE or TSO. Other targets
require zero flags, except Xtensa, whose flags are not checked.

### Constructors and destructors

The loader runs no constructors or destructors, and rejects an image that has
`DT_INIT`, `DT_FINI`, or a non-empty `DT_INIT_ARRAY`, `DT_FINI_ARRAY` or
`DT_PREINIT_ARRAY` with `Error::Constructors`, before it allocates anything.
Initialize state in `_xila_start` instead (for example, avoid
`__attribute__((constructor))` and C++ static initializers). Running
`__init_array` once before the entry point would need the loader to call
through a relocated array, so this is a deliberate omission, not an oversight.

## Building an image

C applications normally use the [SDK](../abi/application/sdk/README.md), which wraps
everything below in a CMake toolchain file. To link by hand, use `xila-static-pie.ld` and define the C entry
`int32_t _xila_start(const XilaStartup *startup)`. The entry receives borrowed
[application ABI capabilities](../abi/application/README.md). Emit a
`PT_NOTE` record named `XILA\0` of type 1 with a four-byte version descriptor
in ELF byte order. The descriptor holds the major version in its high 16 bits
and minor version in its low 16 bits. ABI 0.x requires an exact match. The
[fixture](fixtures/start.c) shows how to take the version from the C header.

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
that holds the image, the process `XilaHeap` granted through `startup->heap` and
the host state: the `XilaStartup` block, the process capability and the
arguments and startup streams they point into. The coroutine entry receives
only a pointer to it, so a program that overruns its own frames cannot damage the
state `xila_process_exit` and the SDK's heap and standard error access depend
on, and the host can reach the streams whatever the coroutine was doing. The
allocation is released when the program returns or when the task is dropped. A
cancelled coroutine's stack stays leaked, as stackful documents, and its
streams are closed only when that allocation is dropped; closing them
earlier and releasing the pipes a cancelled process holds is
[Core #731](https://github.com/xila-project/core/issues/731).

The platform implements `ExecutableMemory`, including cache synchronization
and executable permissions appropriate to its hardware
([Platforms #115](https://github.com/xila-project/platforms/issues/115)).
The current trait requires one contiguous address range for both instruction
fetches and data access. The loader recognizes Xtensa ELF images, and running
them on the coroutine backend is tracked in
[Core #648](https://github.com/xila-project/core/issues/648).

The loader does not log. Read the allocation base with `LoadedImage::base()`
and pass it to GDB as `add-symbol-file <elf> -o <base>`: the provided linker
script links at address zero. Automatic GDB registration is tracked in
[Core #722](https://github.com/xila-project/core/issues/722). Image size is the contiguous virtual span including BSS,
gaps and alignment padding. The raw-file handler holds the original file buffer
until installation finishes. Running adds the configurable coroutine stack and
64-byte guard, the stackful control allocation, and the normal task and
startup allocations.
