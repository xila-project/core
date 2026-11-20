#!/usr/bin/env python3
"""Assert that an SDK-built executable is something the Xila native loader accepts.

    python tools/sdk/check_elf.py IMAGE.elf [--machine arm|x86_64|riscv32|aarch64]

The checks mirror the loader (libraries/native_loader) and the SDK contract:

* ET_DYN with a PT_DYNAMIC segment, no PT_INTERP, no PT_TLS;
* exactly one PT_NOTE record "XILA" type 1 with a four-byte ABI version the host loader
  accepts (every note of every PT_NOTE is read, as the loader does);
* only the dynamic tags the loader consumes or ignores (libraries/native_loader/src/elf.rs):
  anything else, such as DT_RELR from -z pack-relative-relocs, fails the load; no DT_NEEDED,
  DT_INIT, DT_FINI or non-empty init/fini/preinit arrays (the loader runs no constructors);
* every dynamic relocation is the machine's RELATIVE type (no symbols);
* no R_RISCV_RELAX in any relocation section, which would mean the link relaxed against
  the firmware's global pointer.

Only the standard library is used, so the check runs anywhere Python does.
"""

import argparse
import re
import struct
import sys
from pathlib import Path

ET_DYN = 3
PT_DYNAMIC, PT_INTERP, PT_NOTE, PT_TLS = 2, 3, 4, 7
SHT_REL, SHT_RELA = 9, 4

DT_NEEDED, DT_PLTRELSZ, DT_PLTGOT, DT_HASH = 1, 2, 3, 4
DT_RELA, DT_RELASZ, DT_REL, DT_RELSZ = 7, 8, 17, 18
DT_INIT, DT_FINI = 12, 13
DT_INIT_ARRAY, DT_FINI_ARRAY, DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ = 25, 26, 27, 28
DT_PREINIT_ARRAY, DT_PREINIT_ARRAYSZ = 32, 33
DT_JMPREL, DT_TEXTREL = 23, 22
DT_RELRSZ, DT_RELR, DT_RELRENT = 35, 36, 37
DT_RELA_ENTRY, DT_REL_ENTRY = 9, 19  # DT_RELAENT, DT_RELENT

# The loader's dynamic-tag table (Elf::parse_dynamic in libraries/native_loader/src/elf.rs).
# Keep it identical: a tag outside it makes the loader fail the whole image.
LOADER_CONSUMED = {DT_RELA, DT_RELASZ, DT_RELA_ENTRY, DT_REL, DT_RELSZ, DT_REL_ENTRY}
LOADER_IGNORED = (
    set(range(4, 7)) | set(range(10, 17)) | {21, 22} | set(range(24, 31)) | {32, 33}
    | {0x6FFFFEF5, 0x6FFFFFF0} | set(range(0x6FFFFFF9, 0x6FFFFFFE))
)
# Ignored only while zero: a nonzero value means constructors or destructors.
LOADER_CONSTRUCTOR_VALUES = {DT_INIT, DT_FINI, DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ, DT_PREINIT_ARRAYSZ}

EM = {3: "i386", 40: "arm", 62: "x86_64", 183: "aarch64", 243: "riscv"}
RELATIVE = {"arm": 23, "x86_64": 8, "aarch64": 1027, "riscv": 3}
R_RISCV_RELAX = 51
XILA_NOTE_NAME = b"XILA\0"


class ElfError(Exception):
    pass


def host_abi():
    """(major, minor) of the application ABI in this checkout."""
    header = Path(__file__).resolve().parents[2] / "libraries/abi/application/include/xila/application.h"
    text = header.read_text()
    return tuple(int(re.search(rf"XILA_ABI_VERSION_{part}\s+(\d+)", text).group(1))
                 for part in ("MAJOR", "MINOR"))


def abi_accepted(version, host):
    """The loader's compatibility rule (Elf::check_abi)."""
    major, minor = version >> 16, version & 0xFFFF
    return major == host[0] and minor <= host[1] and (major != 0 or minor == host[1])


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
        raise ElfError("PT_DYNAMIC has no DT_NULL terminator")

    def notes(self, segment):
        """(name, type, descriptor) of every note record in a PT_NOTE segment, as the loader
        reads them: four-byte aligned names and descriptors, truncation is an error."""
        data = self.data[segment["offset"]: segment["offset"] + segment["filesz"]]
        while data:
            if len(data) < 12:
                raise ElfError("PT_NOTE ends inside a note header")
            namesz, descsz, kind = struct.unpack_from("<III", data, 0)
            name_end, desc_end = -(-namesz // 4) * 4, -(-descsz // 4) * 4
            if 12 + name_end + desc_end > len(data):
                raise ElfError("PT_NOTE ends inside a note")
            name = data[12: 12 + namesz]
            descriptor = data[12 + name_end: 12 + name_end + descsz]
            yield name, kind, descriptor
            data = data[12 + name_end + desc_end:]

    def virtual_to_offset(self, address):
        for segment in self.segments():
            if segment["type"] == 1 and segment["vaddr"] <= address < segment["vaddr"] + segment["filesz"]:
                return segment["offset"] + address - segment["vaddr"]
        raise ElfError(f"address {address:#x} is outside every PT_LOAD segment")


def check(data, machine=None, abi=None):
    """Return a list of problems; empty means the image passes. `abi` is the host's
    (major, minor), by default the one in this checkout."""
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

    versions = []
    for note in (segment for segment in segments if segment["type"] == PT_NOTE):
        try:
            for name, kind, descriptor in elf.notes(note):
                if kind == 1 and name == XILA_NOTE_NAME:
                    if len(descriptor) != 4:
                        problems.append('the "XILA" note must carry a 4-byte version')
                    else:
                        versions.append(struct.unpack("<I", descriptor)[0])
        except ElfError as error:
            problems.append(str(error))
    if not versions:
        problems.append('no PT_NOTE record named "XILA" of type 1 with a 4-byte version')
    elif len(versions) > 1:
        problems.append('more than one "XILA" ABI note')
    else:
        host = abi or host_abi()
        if not abi_accepted(versions[0], host):
            problems.append(f"ABI {versions[0] >> 16}.{versions[0] & 0xFFFF} is not accepted by "
                            f"the {host[0]}.{host[1]} loader")

    relocation = RELATIVE.get(elf.arch)
    if dynamic:
        seen = set()
        for tag, value in elf.dynamic(dynamic[0]):
            if tag == DT_NEEDED:
                problems.append("DT_NEEDED present: the image must be self-contained")
            elif tag in LOADER_CONSTRUCTOR_VALUES and value != 0:
                problems.append("constructor tag (DT_INIT/DT_FINI or a non-empty init/fini/preinit "
                                "array): the loader runs no constructors")
            elif tag in (DT_JMPREL, DT_PLTRELSZ, DT_PLTGOT):
                problems.append("PLT relocations present: link with -Bsymbolic and no external symbols")
            elif tag == DT_TEXTREL:
                if elf.arch != "xtensa":
                    problems.append("DT_TEXTREL present: text relocations are not supported")
            elif tag in LOADER_CONSUMED:
                if tag in seen:
                    problems.append(f"dynamic tag {tag} appears twice")
                seen.add(tag)
            elif tag not in LOADER_IGNORED:
                hint = (" (-z pack-relative-relocs)" if tag in (DT_RELR, DT_RELRSZ, DT_RELRENT) else "")
                problems.append(f"dynamic tag {tag:#x}{hint} is not supported by the loader")

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
    parser.add_argument("--abi", help="host ABI MAJOR.MINOR (default: from application.h)")
    args = parser.parse_args()
    abi = tuple(int(part) for part in args.abi.split(".")) if args.abi else None
    with open(args.image, "rb") as handle:
        data = handle.read()
    try:
        problems = check(data, args.machine, abi)
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
