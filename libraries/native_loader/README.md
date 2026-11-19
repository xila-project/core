# Native executable loader

The loader installs trusted static PIEs in contiguous executable RAM or PSRAM.
It checks the ELF header against the target, copies `PT_LOAD` segments, zeros
BSS and gaps, and applies relative `DT_REL` and `DT_RELA` relocations. On
Xtensa, a zero explicit RELA addend selects the in-place addend in the data or
instruction literal. The relocation can target an executable segment regardless
of the presence of `DT_TEXTREL`. The loader rejects interpreters, dynamic
dependencies, native ELF TLS, and symbol-based relocations.

## Building an image

Use `xila-static-pie.ld` and define the C entry
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
`sh fixtures/build.sh <output>` from this directory. It uses a fixture-only
callback argument while the startup API table is being introduced in #684.
Run `mise run test:native-loader` from the repository root.

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
arguments and leaves the optional home capability unset. The coroutine owns
the image through return. Cancelling a suspended coroutine retains its stack
and the image in accordance with stackful's cancellation contract.

The platform implements `ExecutableMemory`, including cache synchronization
and executable permissions appropriate to its hardware
([Platforms #115](https://github.com/xila-project/platforms/issues/115)).
The current trait requires one contiguous address range for both instruction
fetches and data access. The loader recognizes Xtensa ELF images, and running
them on the coroutine backend is tracked in
[Core #648](https://github.com/xila-project/core/issues/648).

The loader emits the allocation base and the GDB `add-symbol-file` offset at
debug log level. Automatic GDB registration is tracked in
[Core #722](https://github.com/xila-project/core/issues/722). Image size is the contiguous virtual span including BSS,
gaps and alignment padding. The raw-file handler holds the original file buffer
until installation finishes. Running adds the configurable coroutine stack and
64-byte guard, the stackful control allocation, and the normal task and
startup allocations.
