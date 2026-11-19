#!/usr/bin/env python3
"""Assert that an SDK-built executable is something the Xila native loader accepts.

    python tools/sdk/check_elf.py IMAGE.elf [--machine arm|x86_64|riscv32|aarch64]

The checks mirror the loader (libraries/native_loader) and the SDK contract:

* ET_DYN with a PT_DYNAMIC segment, no PT_INTERP, no PT_TLS;
* the PT_NOTE record "XILA" type 1 with a four-byte ABI version;
* no DT_NEEDED, DT_INIT, DT_FINI or non-empty init/fini/preinit arrays (the loader runs
  no constructors);
* every dynamic relocation is the machine's RELATIVE type (no symbols);
* no R_RISCV_RELAX in any relocation section, which would mean the link relaxed against
  the firmware's global pointer.

Only the standard library is used, so the check runs anywhere Python does.
"""

import argparse
import struct
import sys

ET_DYN = 3
PT_DYNAMIC, PT_INTERP, PT_NOTE, PT_TLS = 2, 3, 4, 7
SHT_REL, SHT_RELA = 9, 4

DT_NEEDED, DT_PLTRELSZ, DT_PLTGOT, DT_HASH = 1, 2, 3, 4
DT_RELA, DT_RELASZ, DT_REL, DT_RELSZ = 7, 8, 17, 18
DT_INIT, DT_FINI = 12, 13
DT_INIT_ARRAY, DT_FINI_ARRAY, DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ = 25, 26, 27, 28
DT_PREINIT_ARRAY, DT_PREINIT_ARRAYSZ = 32, 33
DT_JMPREL, DT_TEXTREL = 23, 22

EM = {3: "i386", 40: "arm", 62: "x86_64", 183: "aarch64", 243: "riscv"}
RELATIVE = {"arm": 23, "x86_64": 8, "aarch64": 1027, "riscv": 3}
R_RISCV_RELAX = 51
XILA_NOTE_NAME = b"XILA\0"


class ElfError(Exception):
    pass


class Elf:
    def __init__(self, data):
        if data[:4] != b"\x7fELF":
            raise ElfError("not an ELF file")
        self.data = data
        self.is64 = data[4] == 2
        if data[5] != 1:
            raise ElfError("only little-endian images are supported")
        e = "<"
        if self.is64:
            (self.type, self.machine, _, self.entry, self.phoff, self.shoff, _flags, _eh,
             self.phentsize, self.phnum, self.shentsize, self.shnum,
             self.shstrndx) = struct.unpack_from(e + "HHIQQQIHHHHHH", data, 16)
        else:
            (self.type, self.machine, _, self.entry, self.phoff, self.shoff, _flags, _eh,
             self.phentsize, self.phnum, self.shentsize, self.shnum,
             self.shstrndx) = struct.unpack_from(e + "HHIIIIIHHHHHH", data, 16)
        self.arch = EM.get(self.machine, f"machine-{self.machine}")

    def segments(self):
        for index in range(self.phnum):
            base = self.phoff + index * self.phentsize
            if self.is64:
                kind, flags, offset, vaddr, _p, filesz, memsz, align = struct.unpack_from(
                    "<IIQQQQQQ", self.data, base)
            else:
                kind, offset, vaddr, _p, filesz, memsz, flags, align = struct.unpack_from(
                    "<IIIIIIII", self.data, base)
            yield dict(type=kind, flags=flags, offset=offset, vaddr=vaddr, filesz=filesz,
                       memsz=memsz, align=align)

    def sections(self):
        for index in range(self.shnum):
            base = self.shoff + index * self.shentsize
            if self.is64:
                name, kind, _f, _a, offset, size, _l, _i, _al, entsize = struct.unpack_from(
                    "<IIQQQQIIQQ", self.data, base)
            else:
                name, kind, _f, _a, offset, size, _l, _i, _al, entsize = struct.unpack_from(
                    "<IIIIIIIIII", self.data, base)
            yield dict(name=name, type=kind, offset=offset, size=size, entsize=entsize)

    def relocations(self, section):
        """(type, symbol) of each entry in a SHT_REL or SHT_RELA section."""
        size = (24 if self.is64 else 12) if section["type"] == SHT_RELA else (16 if self.is64 else 8)
        for offset in range(section["offset"], section["offset"] + section["size"], size):
            if self.is64:
                info = struct.unpack_from("<Q", self.data, offset + 8)[0]
                yield info & 0xFFFFFFFF, info >> 32
            else:
                info = struct.unpack_from("<I", self.data, offset + 4)[0]
                yield info & 0xFF, info >> 8

    def dynamic(self, segment):
        entry = 16 if self.is64 else 8
        fmt = "<qQ" if self.is64 else "<iI"
        for offset in range(segment["offset"], segment["offset"] + segment["filesz"], entry):
            tag, value = struct.unpack_from(fmt, self.data, offset)
            if tag == 0:
                return
            yield tag, value

    def virtual_to_offset(self, address):
        for segment in self.segments():
            if segment["type"] == 1 and segment["vaddr"] <= address < segment["vaddr"] + segment["filesz"]:
                return segment["offset"] + address - segment["vaddr"]
        raise ElfError(f"address {address:#x} is outside every PT_LOAD segment")


def check(data, machine=None):
    """Return a list of problems; empty means the image passes."""
    elf = Elf(data)
    problems = []
    if machine and elf.arch != machine and not (machine == "riscv32" and elf.arch == "riscv"):
        problems.append(f"machine is {elf.arch}, expected {machine}")
    if elf.type != ET_DYN:
        problems.append(f"e_type is {elf.type}, expected ET_DYN (static-PIE)")
    segments = list(elf.segments())
    kinds = [segment["type"] for segment in segments]
    if PT_INTERP in kinds:
        problems.append("PT_INTERP present (link with --no-dynamic-linker, never -static-pie on Xtensa)")
    if PT_TLS in kinds:
        problems.append("PT_TLS present (picolibc must be built without thread-local storage)")
    dynamic = [segment for segment in segments if segment["type"] == PT_DYNAMIC]
    if len(dynamic) != 1:
        problems.append("expected exactly one PT_DYNAMIC segment")

    notes = [s for s in segments if s["type"] == PT_NOTE]
    found_note = False
    for note in notes:
        namesz, descsz, kind = struct.unpack_from("<III", data, note["offset"])
        name = data[note["offset"] + 12: note["offset"] + 12 + namesz]
        if name == XILA_NOTE_NAME and kind == 1 and descsz == 4:
            found_note = True
    if not found_note:
        problems.append('no PT_NOTE record named "XILA" of type 1 with a 4-byte version')

    relocation = RELATIVE.get(elf.arch)
    if dynamic:
        values = {}
        for tag, value in elf.dynamic(dynamic[0]):
            if tag == DT_NEEDED:
                problems.append("DT_NEEDED present: the image must be self-contained")
            if tag in (DT_INIT, DT_FINI):
                problems.append("DT_INIT/DT_FINI present: the loader runs no constructors")
            if tag in (DT_JMPREL, DT_PLTRELSZ, DT_PLTGOT):
                problems.append("PLT relocations present: link with -Bsymbolic and no external symbols")
            if tag == DT_TEXTREL and elf.arch != "xtensa":
                problems.append("DT_TEXTREL present: text relocations are not supported")
            values[tag] = values.get(tag, 0) + value if tag in (DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ,
                                                                DT_PREINIT_ARRAYSZ) else value
        for tag in (DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ, DT_PREINIT_ARRAYSZ):
            if values.get(tag, 0) != 0:
                problems.append("non-empty init/fini/preinit array: the loader runs no constructors")

    for section in elf.sections():
        if section["type"] not in (SHT_REL, SHT_RELA):
            continue
        for kind, symbol in elf.relocations(section):
            if elf.arch == "riscv" and kind == R_RISCV_RELAX:
                problems.append("R_RISCV_RELAX present: link with -mno-relax and --no-relax")
            elif relocation is not None and kind != relocation:
                problems.append(f"relocation type {kind} is not RELATIVE ({relocation})")
            elif symbol != 0:
                problems.append("a relocation refers to a symbol: the image has external references")
    return sorted(set(problems))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("image")
    parser.add_argument("--machine", choices=["arm", "x86_64", "riscv32", "aarch64"])
    args = parser.parse_args()
    with open(args.image, "rb") as handle:
        data = handle.read()
    try:
        problems = check(data, args.machine)
    except ElfError as error:
        problems = [str(error)]
    for problem in problems:
        print(f"{args.image}: {problem}", file=sys.stderr)
    if problems:
        sys.exit(1)
    elf = Elf(data)
    relative = sum(
        1 for section in elf.sections() if section["type"] in (SHT_REL, SHT_RELA)
        for _ in elf.relocations(section))
    print(f"{args.image}: ok ({elf.arch}, ET_DYN, no PT_INTERP, {relative} RELATIVE relocations)")


if __name__ == "__main__":
    main()
