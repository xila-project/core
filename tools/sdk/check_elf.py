#!/usr/bin/env python3
"""Assert that an SDK-built executable is something the Xila native loader accepts.

    python tools/sdk/check_elf.py IMAGE.elf [--machine arm|x86_64|riscv32|aarch64|xtensa]
        [--float-abi soft|softfp|single|double|hard] [--no-compressed] [--embedded]

The checks mirror the loader (libraries/native_loader) and the SDK contract:

* ET_DYN with a PT_DYNAMIC segment inside a PT_LOAD, no PT_INTERP, no PT_TLS;
* the header, program header and PT_LOAD alignment rules of Elf::parse, an entry point in an
  executable PT_LOAD (a Thumb entry on Arm), and the `e_flags` the loader accepts: Arm
  EABI5 with the target's float ABI, RISC-V float ABI and RVC/RVE flags, zero on x86-64 and
  AArch64 (Xtensa flags are not checked, as in the loader);
* exactly one PT_NOTE record "XILA" type 1 with a four-byte Xila version,
  `(major << 16) | minor`, that the host loader accepts (every note of every PT_NOTE is
  read, as the loader does);
* only the dynamic tags the loader consumes or ignores (libraries/native_loader/src/elf.rs):
  anything else, such as DT_RELR from -z pack-relative-relocs, fails the load; no DT_NEEDED,
  DT_INIT, DT_FINI or non-empty init/fini/preinit arrays (the loader runs no constructors);
* DT_REL/DT_RELA tables that are complete, with the entry size the loader expects;
* every dynamic relocation is the machine's RELATIVE type (no symbols);
* no gp relaxation on RISC-V: an image must not define `__global_pointer$`. lld relaxes
  gp-relative accesses only with --relax-gp and that symbol, and the image's gp is not the
  firmware's. Call relaxation (pc-relative jal/c.jal) is valid in a PIE and stays on. The
  check reads the symbol tables, so a stripped image passes it vacuously.

Only the standard library is used, so the check runs anywhere Python does.
"""

import argparse
import re
import struct
import sys
from pathlib import Path

ET_DYN = 3
PT_LOAD, PT_DYNAMIC, PT_INTERP, PT_NOTE, PT_TLS = 1, 2, 3, 4, 7
PF_X, PF_W = 1, 2
# The loader's limits (libraries/native_loader/src/elf.rs).
MAX_ALIGNMENT = 64 * 1024
MAX_PROGRAM_HEADERS = 16
SHT_REL, SHT_RELA = 9, 4
SHT_SYMTAB, SHT_DYNSYM, SHT_STRTAB = 2, 11, 3
SHN_UNDEF = 0
GLOBAL_POINTER = b"__global_pointer$"

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
DT_XTENSA_GOT_LOC_OFF, DT_XTENSA_GOT_LOC_SZ = 0x7000_0000, 0x7000_0001
XTENSA_IGNORED_DYNAMIC = {DT_PLTGOT, DT_XTENSA_GOT_LOC_OFF, DT_XTENSA_GOT_LOC_SZ}
# Ignored only while zero: a nonzero value means constructors or destructors.
LOADER_CONSTRUCTOR_VALUES = {DT_INIT, DT_FINI, DT_INIT_ARRAYSZ, DT_FINI_ARRAYSZ, DT_PREINIT_ARRAYSZ}

EM = {3: "i386", 40: "arm", 62: "x86_64", 94: "xtensa", 183: "aarch64", 243: "riscv"}
RELATIVE = {"arm": 23, "x86_64": 8, "aarch64": 1027, "riscv": 3, "xtensa": 5}

# `e_flags` (arm_flags_supported and riscv_flags_supported in the loader).
EF_ARM_BE8, EF_ARM_ABI_FLOAT_SOFT, EF_ARM_ABI_FLOAT_HARD = 0x0080_0000, 0x200, 0x400
EF_RISCV_RVC, EF_RISCV_FLOAT_ABI, EF_RISCV_RVE = 0x1, 0x6, 0x8
RISCV_FLOAT_ABI = {"soft": 0, "single": 2, "double": 4}
# The float ABIs a target can name, by architecture. "softfp" is the base (soft-float
# calling convention) variant on an FPU, which the Arm flags cannot tell from "soft".
# Other architectures have no float ABI to name.
FLOAT_ABIS_BY_ARCH = {
    "arm": ("soft", "softfp", "hard"),
    "riscv": ("soft", "single", "double"),
}
FLOAT_ABIS = ("soft", "softfp", "single", "double", "hard")
XILA_NOTE_NAME = b"XILA\0"


class ElfError(Exception):
    pass


def host_xila_version():
    """(major, minor) of the Xila version in this checkout, as the generated header states it."""
    header = Path(__file__).resolve().parents[2] / "libraries/abi/application/include/xila/application.h"
    text = header.read_text()
    return tuple(int(re.search(rf"^#define XILA_VERSION_{part}\s+(\d+)", text, re.M).group(1))
                 for part in ("MAJOR", "MINOR"))


def version_accepted(version, host):
    """The loader's compatibility rule (xila_version::compatible, which mirrors
    xila_package_manifest::compatible): before 1.0 the minor versions must be equal; from 1.0
    the majors must be equal and the host minor at least the one the image was built for."""
    major, minor = version >> 16, version & 0xFFFF
    if major != host[0]:
        return False
    return minor == host[1] if major == 0 else minor <= host[1]


class Elf:
    def __init__(self, data):
        if data[:4] != b"\x7fELF":
            raise ElfError("not an ELF file")
        if len(data) < 6:
            raise ElfError("truncated ELF: no class and byte order")
        if data[4] not in (1, 2):
            raise ElfError(f"unknown ELF class {data[4]}")
        self.data = data
        self.is64 = data[4] == 2
        if data[5] != 1:
            raise ElfError("only little-endian images are supported")
        header_size = 64 if self.is64 else 52
        if len(data) < header_size:
            raise ElfError(f"truncated ELF: {len(data)} bytes, the header takes {header_size}")
        e = "<"
        if self.is64:
            (self.type, self.machine, self.version, self.entry, self.phoff, self.shoff, self.flags, self.ehsize,
             self.phentsize, self.phnum, self.shentsize, self.shnum,
             self.shstrndx) = struct.unpack_from(e + "HHIQQQIHHHHHH", data, 16)
        else:
            (self.type, self.machine, self.version, self.entry, self.phoff, self.shoff, self.flags, self.ehsize,
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
                name, kind, _f, _a, offset, size, link, _i, _al, entsize = struct.unpack_from(
                    "<IIQQQQIIQQ", self.data, base)
            else:
                name, kind, _f, _a, offset, size, link, _i, _al, entsize = struct.unpack_from(
                    "<IIIIIIIIII", self.data, base)
            yield dict(name=name, type=kind, offset=offset, size=size, link=link, entsize=entsize)

    def defined_symbols(self):
        """Names of the defined symbols in every SHT_SYMTAB and SHT_DYNSYM section."""
        sections = list(self.sections())
        size = 24 if self.is64 else 16
        for section in sections:
            if section["type"] not in (SHT_SYMTAB, SHT_DYNSYM):
                continue
            if section["link"] >= len(sections) or sections[section["link"]]["type"] != SHT_STRTAB:
                raise ElfError("a symbol table has no string table")
            strings = sections[section["link"]]
            table = self.data[strings["offset"]: strings["offset"] + strings["size"]]
            for offset in range(section["offset"], section["offset"] + section["size"] - size + 1, size):
                if self.is64:
                    name, _info, _other, shndx = struct.unpack_from("<IBBH", self.data, offset)
                else:
                    name, _value, _size, _info, _other, shndx = struct.unpack_from(
                        "<IIIBBH", self.data, offset)
                if shndx == SHN_UNDEF or name >= len(table):
                    continue
                end = table.find(b"\0", name)
                yield table[name: end if end >= 0 else len(table)]

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
            if offset + entry > len(self.data):
                raise ElfError("PT_DYNAMIC ends inside an entry")
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


def float_abi_problem(elf, float_abi):
    """Why `float_abi` cannot describe an image of this architecture, or None."""
    if float_abi is None:
        return None
    valid = FLOAT_ABIS_BY_ARCH.get(elf.arch)
    if valid is None:
        return f"float ABI {float_abi!r} does not apply to {elf.arch}, which has none to name"
    if float_abi not in valid:
        return f"float ABI {float_abi!r} is not one of {'/'.join(valid)} on {elf.arch}"
    return None


def flags_problem(elf, float_abi, compressed, embedded):
    """Why the loader would reject `e_flags`, or None. Mirrors Native::flags_supported.
    `float_abi` is the target's ("soft", "softfp" or "hard" on Arm, "soft", "single" or
    "double" on RISC-V; see float_abi_problem); None skips the float ABI comparison."""
    flags = elf.flags
    if elf.arch == "arm":
        if flags >> 24 != 5:
            return f"e_flags {flags:#x}: not EABI version 5"
        if flags & EF_ARM_BE8:
            return f"e_flags {flags:#x}: big-endian code (EF_ARM_BE8)"
        float_flags = flags & (EF_ARM_ABI_FLOAT_SOFT | EF_ARM_ABI_FLOAT_HARD)
        # An image that sets neither float ABI flag runs on both.
        allowed = {0, EF_ARM_ABI_FLOAT_SOFT, EF_ARM_ABI_FLOAT_HARD}
        if float_abi == "hard":
            allowed = {0, EF_ARM_ABI_FLOAT_HARD}
        elif float_abi in ("soft", "softfp"):
            allowed = {0, EF_ARM_ABI_FLOAT_SOFT}
        if float_flags not in allowed:
            wanted = f", expected the {float_abi}-float ABI" if float_abi else ""
            return f"e_flags {flags:#x}: float ABI flags {float_flags:#x} are not supported{wanted}"
    elif elf.arch == "riscv":
        if flags & ~(EF_RISCV_RVC | EF_RISCV_FLOAT_ABI | EF_RISCV_RVE):
            return f"e_flags {flags:#x}: unsupported RISC-V flag"
        if flags & EF_RISCV_RVC and not compressed:
            return f"e_flags {flags:#x}: compressed instructions on a target without them"
        if float_abi is not None and flags & EF_RISCV_FLOAT_ABI != RISCV_FLOAT_ABI.get(float_abi):
            return f"e_flags {flags:#x}: float ABI is not {float_abi}"
        if bool(flags & EF_RISCV_RVE) != embedded:
            return f"e_flags {flags:#x}: RVE does not match the target"
    elif elf.arch in ("x86_64", "aarch64"):
        if flags != 0:
            return f"e_flags {flags:#x}: must be zero"
    # Xtensa flags describe the core configuration, which the linker checks.
    return None


def instruction_alignment(elf):
    if elf.arch == "aarch64":
        return 4
    if elf.arch == "arm":
        return 4 if elf.entry & 1 == 0 else 2
    return 2 if elf.arch == "riscv" else 1


def check(data, machine=None, xila_version=None, float_abi=None, compressed=True, embedded=False):
    """Return a list of problems; empty means the image passes. `xila_version` is the host's
    Xila (major, minor), by default the one in this checkout. `float_abi`, `compressed` and
    `embedded` describe the target for the `e_flags` check (see flags_problem). A truncated
    image raises ElfError."""
    try:
        return _check(data, machine, xila_version, float_abi, compressed, embedded)
    except struct.error as error:
        raise ElfError(f"truncated ELF: {error}") from error


def _check(data, machine, xila_version, float_abi, compressed, embedded):
    elf = Elf(data)
    problems = []
    actual_machine = f"riscv{64 if elf.is64 else 32}" if elf.arch == "riscv" else elf.arch
    if machine and machine not in (elf.arch, actual_machine):
        problems.append(f"machine is {actual_machine}, expected {machine}")
    else:
        # A float ABI that its architecture does not have is a usage error, not a
        # problem with the image.
        usage = float_abi_problem(elf, float_abi)
        if usage:
            raise ElfError(usage)
    if elf.type != ET_DYN:
        problems.append(f"e_type is {elf.type}, expected ET_DYN (static-PIE)")
    if elf.version != 1:
        problems.append(f"e_version is {elf.version}, expected 1")
    problem = flags_problem(elf, float_abi, compressed, embedded)
    if problem:
        problems.append(problem)
    phentsize = 56 if elf.is64 else 32
    if elf.phentsize != phentsize:
        problems.append(f"e_phentsize is {elf.phentsize}, expected {phentsize}")
    if not 1 <= elf.phnum <= MAX_PROGRAM_HEADERS:
        problems.append(f"{elf.phnum} program headers: the loader accepts 1 to {MAX_PROGRAM_HEADERS}")
    segments = list(elf.segments())
    kinds = [segment["type"] for segment in segments]
    if PT_INTERP in kinds:
        problems.append("PT_INTERP present (link with --no-dynamic-linker, never -static-pie on Xtensa)")
    if PT_TLS in kinds:
        problems.append("PT_TLS present (picolibc must be built without thread-local storage)")
    dynamic = [segment for segment in segments if segment["type"] == PT_DYNAMIC]
    if len(dynamic) != 1:
        problems.append("expected exactly one PT_DYNAMIC segment")

    loads = [segment for segment in segments if segment["type"] == PT_LOAD]
    for segment in loads:
        align = segment["align"]
        if align > 1 and (align & (align - 1) != 0 or align > MAX_ALIGNMENT
                          or segment["offset"] % align != segment["vaddr"] % align):
            problems.append(f"PT_LOAD alignment {align:#x} is not a power of two up to "
                            f"{MAX_ALIGNMENT:#x} congruent with its file offset")
        if segment["filesz"] > segment["memsz"] or segment["offset"] + segment["filesz"] > len(data):
            problems.append("a PT_LOAD segment is larger in the file than in memory, or outside the file")

    def load_for(address, size):
        """The PT_LOAD whose file bytes hold [address, address + size), as Elf::file_range."""
        for segment in loads:
            if (segment["memsz"] != 0 and segment["vaddr"] <= address
                    and address - segment["vaddr"] + size <= segment["filesz"]
                    and segment["offset"] + address - segment["vaddr"] + size <= len(data)):
                return segment
        return None

    code = elf.entry & ~1 if elf.arch == "arm" else elf.entry
    ialign = instruction_alignment(elf)
    if elf.arch == "arm" and elf.entry & 1 == 0:
        problems.append("the entry point is not a Thumb address (bit 0 clear): "
                        "the SDK's Arm targets run Thumb only")
    holder = load_for(code, ialign)
    if holder is None or not holder["flags"] & PF_X or code % ialign != 0:
        problems.append("the entry point is not an aligned address inside an executable PT_LOAD segment")

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
        host = xila_version or host_xila_version()
        if not version_accepted(versions[0], host):
            problems.append(f"built for Xila {versions[0] >> 16}.{versions[0] & 0xFFFF}, which is "
                            f"not accepted by the Xila {host[0]}.{host[1]} loader")

    relocation = RELATIVE.get(elf.arch)
    if dynamic:
        word = 8 if elf.is64 else 4
        segment = dynamic[0]
        holder = load_for(segment["vaddr"], segment["filesz"])
        if (segment["filesz"] == 0 or segment["filesz"] > segment["memsz"]
                or segment["filesz"] % (2 * word) != 0 or holder is None
                or holder["offset"] + segment["vaddr"] - holder["vaddr"] != segment["offset"]):
            problems.append("PT_DYNAMIC is empty, not a whole number of entries, or not backed by "
                            "the same bytes of a PT_LOAD segment")
        seen = set()
        tables = {}
        for tag, value in elf.dynamic(segment):
            if tag == DT_NEEDED:
                problems.append("DT_NEEDED present: the image must be self-contained")
            elif tag in LOADER_CONSTRUCTOR_VALUES and value != 0:
                problems.append("constructor tag (DT_INIT/DT_FINI or a non-empty init/fini/preinit "
                                "array): the loader runs no constructors")
            elif tag in (DT_JMPREL, DT_PLTRELSZ) or (tag == DT_PLTGOT and elf.arch != "xtensa"):
                problems.append("PLT relocations present: link with -Bsymbolic and no external symbols")
            elif elf.arch == "xtensa" and tag in XTENSA_IGNORED_DYNAMIC:
                continue
            elif tag == DT_TEXTREL:
                if elf.arch != "xtensa":
                    problems.append("DT_TEXTREL present: text relocations are not supported")
            elif tag in LOADER_CONSUMED:
                if tag in seen:
                    problems.append(f"dynamic tag {tag} appears twice")
                seen.add(tag)
                tables[tag] = value
            elif tag not in LOADER_IGNORED:
                hint = (" (-z pack-relative-relocs)" if tag in (DT_RELR, DT_RELRSZ, DT_RELRENT) else "")
                problems.append(f"dynamic tag {tag:#x}{hint} is not supported by the loader")
        # Table::bytes: address, size and entry size come together, and the entry size is the
        # one the loader reads (REL: two words, RELA: three).
        for name, parts, stride in (("DT_REL", (DT_REL, DT_RELSZ, DT_REL_ENTRY), 2 * word),
                                    ("DT_RELA", (DT_RELA, DT_RELASZ, DT_RELA_ENTRY), 3 * word)):
            present = [part in tables for part in parts]
            if not any(present):
                continue
            address, size, entry_size = (tables.get(part) for part in parts)
            if (not all(present) or entry_size != stride or size % stride != 0
                    or (size != 0 and load_for(address, size) is None)):
                problems.append(f"{name} table is incomplete, has an entry size other than "
                                f"{stride}, a size that is not a whole number of entries, or lies "
                                "outside every PT_LOAD segment")

    for section in elf.sections():
        if section["type"] not in (SHT_REL, SHT_RELA):
            continue
        for kind, symbol in elf.relocations(section):
            if relocation is not None and kind != relocation:
                problems.append(f"relocation type {kind} is not RELATIVE ({relocation})")
            elif symbol != 0:
                problems.append("a relocation refers to a symbol: the image has external references")
    if elf.arch == "riscv" and GLOBAL_POINTER in set(elf.defined_symbols()):
        problems.append("__global_pointer$ is defined: lld would relax gp-relative accesses "
                        "against a gp the firmware does not share (no gp relaxation)")
    return sorted(set(problems))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("image")
    parser.add_argument("--machine", choices=["arm", "x86_64", "riscv32", "aarch64", "xtensa"])
    parser.add_argument("--float-abi", choices=FLOAT_ABIS,
                        help="the target's float ABI, for the e_flags check (Arm: soft, softfp "
                             "or hard; RISC-V: soft, single or double)")
    parser.add_argument("--no-compressed", action="store_true",
                        help="the RISC-V target has no C extension")
    parser.add_argument("--embedded", action="store_true", help="the RISC-V target is RV32E")
    # `--abi` is the earlier name, still passed by the native C application template.
    parser.add_argument("--xila-version", "--abi", dest="xila_version",
                        help="host Xila version MAJOR.MINOR (default: from application.h)")
    args = parser.parse_args()
    xila_version = (tuple(int(part) for part in args.xila_version.split("."))
                    if args.xila_version else None)
    with open(args.image, "rb") as handle:
        data = handle.read()
    try:
        problems = check(data, args.machine, xila_version, args.float_abi,
                         not args.no_compressed, args.embedded)
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
