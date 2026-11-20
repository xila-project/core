"""The ELF checker rejects what the native loader rejects, using synthetic images."""

import os
import shutil
import struct
import subprocess
import sys
import unittest
from pathlib import Path

import check_elf

PT_LOAD = 1
EM_X86_64, EM_RISCV = 62, 243
R_X86_64_RELATIVE, R_X86_64_64 = 8, 1
R_RISCV_RELATIVE = 3
HOST_ABI = (0, 1)
VERSION = struct.pack("<I", (0 << 16) | 1)
FIXTURES = Path(__file__).resolve().parents[2] / "libraries/native_loader/fixtures"


def check(data, machine=None, abi=HOST_ABI):
    """check_elf.check against a fixed host ABI, so the tests do not move with the header."""
    return check_elf.check(data, machine, abi)


def note(name=b"XILA\0", kind=1, descriptor=VERSION):
    padded = name + b"\0" * (-len(name) % 4)
    return struct.pack("<III", len(name), len(descriptor), kind) + padded + descriptor


def image(machine=EM_X86_64, etype=3, extra_segments=(), dynamic=(), relocations=(), notes=None,
          second_notes=None, terminate=True):
    """A 64-bit little-endian ELF with the given program headers and one RELA section.
    `second_notes` adds a second PT_NOTE segment; `terminate=False` omits DT_NULL."""
    note_bytes = note() if notes is None else notes
    dynamic_bytes = b"".join(struct.pack("<qQ", tag, value) for tag, value in dynamic)
    if terminate:
        dynamic_bytes += struct.pack("<qQ", 0, 0)
    rela = b"".join(struct.pack("<QQq", 0x1000 + 8 * index, (symbol << 32) | kind, 0)
                    for index, (kind, symbol) in enumerate(relocations))
    payloads = [dynamic_bytes, note_bytes]
    kinds = [2, 4]
    if second_notes is not None:
        payloads.append(second_notes)
        kinds.append(4)
    segments = [(kind, len(payload)) for kind, payload in zip(kinds, payloads)]
    segments += [(kind, 0) for kind in extra_segments]
    header_size = 64 + 56 * len(segments)
    offsets, cursor = [], header_size
    for payload in payloads:
        offsets.append(cursor)
        cursor += len(payload)
    rela_offset = cursor
    section_offset = rela_offset + len(rela)
    body = b"".join(payloads) + rela
    phdrs = b""
    for index, (kind, size) in enumerate(segments):
        offset = offsets[index] if index < len(payloads) else 0
        phdrs += struct.pack("<IIQQQQQQ", kind, 4, offset, offset, offset, size, size, 8)
    sections = struct.pack("<IIQQQQIIQQ", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    sections += struct.pack("<IIQQQQIIQQ", 0, check_elf.SHT_RELA, 0, 0, rela_offset, len(rela), 0, 0, 8, 24)
    header = b"\x7fELF" + bytes([2, 1, 1, 0]) + bytes(8)
    header += struct.pack("<HHIQQQIHHHHHH", etype, machine, 1, 0, 64, section_offset, 0, 64, 56,
                          len(segments), 64, 2, 0)
    return header + phdrs + body + sections


class CheckElf(unittest.TestCase):
    def test_accepts_a_static_pie_with_relative_relocations(self):
        data = image(relocations=[(R_X86_64_RELATIVE, 0)] * 3, dynamic=[(check_elf.DT_INIT_ARRAYSZ, 0)])
        self.assertEqual(check(data, "x86_64"), [])

    def test_accepts_the_tags_the_loader_ignores(self):
        tags = [(4, 0x100), (5, 0x200), (6, 0x300), (10, 8), (11, 24), (14, 1), (21, 0), (24, 0),
                (25, 0x400), (26, 0x500), (27, 0), (28, 0), (30, 8), (0x6FFFFEF5, 0x600),
                (0x6FFFFFF0, 0x700), (0x6FFFFFF9, 3), (0x6FFFFFFB, 1)]
        self.assertEqual(check(image(dynamic=tags, relocations=[(R_X86_64_RELATIVE, 0)]), "x86_64"), [])

    def test_rejects_a_fixed_executable(self):
        self.assertTrue(any("ET_DYN" in p for p in check(image(etype=2))))

    def test_rejects_an_interpreter_and_tls(self):
        problems = check(image(extra_segments=[check_elf.PT_INTERP, check_elf.PT_TLS]))
        self.assertTrue(any("PT_INTERP" in p for p in problems))
        self.assertTrue(any("PT_TLS" in p for p in problems))

    def test_rejects_a_missing_abi_note(self):
        problems = check(image(notes=note(name=b"GNU\0")))
        self.assertTrue(any("XILA" in p for p in problems))

    def test_rejects_symbol_relocations(self):
        problems = check(image(relocations=[(R_X86_64_64, 3)]))
        self.assertTrue(any("not RELATIVE" in p for p in problems))
        problems = check(image(relocations=[(R_X86_64_RELATIVE, 3)]))
        self.assertTrue(any("symbol" in p for p in problems))

    def test_rejects_constructors_and_dependencies(self):
        for tag in (check_elf.DT_NEEDED, check_elf.DT_INIT, check_elf.DT_FINI):
            self.assertTrue(check(image(dynamic=[(tag, 1)])), tag)
        problems = check(image(dynamic=[(check_elf.DT_INIT_ARRAYSZ, 8)]))
        self.assertTrue(any("init/fini" in p for p in problems))

    def test_rejects_every_dynamic_tag_the_loader_rejects(self):
        # DT_RELR and friends come from -z pack-relative-relocs: the loader does not
        # understand them and fails the image, so the checker must as well.
        for tag in (check_elf.DT_RELRSZ, check_elf.DT_RELR, check_elf.DT_RELRENT, 0x6FFFFEFF,
                    0x7000_0000, 34, 0x6FFFFFFE):
            problems = check(image(dynamic=[(tag, 8)]))
            self.assertTrue(any("not supported by the loader" in p for p in problems), tag)
        problems = check(image(dynamic=[(check_elf.DT_RELR, 0x100)]))
        self.assertTrue(any("pack-relative-relocs" in p for p in problems))

    def test_rejects_a_repeated_relocation_tag(self):
        problems = check(image(dynamic=[(check_elf.DT_RELA, 8), (check_elf.DT_RELA, 16)]))
        self.assertTrue(any("twice" in p for p in problems))

    def test_rejects_an_unterminated_dynamic_section(self):
        with self.assertRaises(check_elf.ElfError):
            check(image(dynamic=[(check_elf.DT_RELA, 8)], terminate=False))

    def test_reads_every_note_of_every_note_segment(self):
        other = note(name=b"GNU\0", kind=3, descriptor=bytes(20))
        # The ABI note after another record in the same segment, and in a second segment.
        self.assertEqual(check(image(notes=other + note())), [])
        self.assertEqual(check(image(notes=other, second_notes=note())), [])

    def test_rejects_a_repeated_abi_note(self):
        self.assertTrue(any("more than one" in p for p in check(image(notes=note() + note()))))
        self.assertTrue(any("more than one" in p for p in check(image(notes=note(), second_notes=note()))))

    def test_rejects_a_malformed_abi_note(self):
        problems = check(image(notes=note(descriptor=struct.pack("<II", 1, 0))))
        self.assertTrue(any("4-byte" in p for p in problems))
        problems = check(image(notes=note()[:-4]))
        self.assertTrue(any("PT_NOTE ends" in p for p in problems))
        self.assertTrue(any("XILA" in p for p in problems))

    def test_checks_the_abi_version_like_the_loader(self):
        def version(major, minor):
            return image(notes=note(descriptor=struct.pack("<I", (major << 16) | minor)))
        self.assertEqual(check(version(0, 1), abi=(0, 1)), [])
        self.assertTrue(any("not accepted" in p for p in check(version(0, 2), abi=(0, 1))))
        self.assertTrue(any("not accepted" in p for p in check(version(0, 1), abi=(0, 2))))
        self.assertTrue(any("not accepted" in p for p in check(version(1, 0), abi=(0, 1))))
        # After 1.0 a program built against an older minor still loads, a newer one does not.
        self.assertEqual(check(version(1, 1), abi=(1, 2)), [])
        self.assertTrue(any("not accepted" in p for p in check(version(1, 3), abi=(1, 2))))

    def test_the_abi_defaults_to_the_headers_in_this_checkout(self):
        major, minor = check_elf.host_abi()
        data = image(notes=note(descriptor=struct.pack("<I", (major << 16) | minor)))
        self.assertEqual(check_elf.check(data, "x86_64"), [])

    def test_rejects_riscv_linker_relaxation(self):
        good = image(machine=EM_RISCV, relocations=[(R_RISCV_RELATIVE, 0)])
        self.assertEqual(check(good, "riscv32"), [])
        bad = image(machine=EM_RISCV, relocations=[(check_elf.R_RISCV_RELAX, 0)])
        self.assertTrue(any("R_RISCV_RELAX" in p for p in check(bad, "riscv32")))

    def test_rejects_the_wrong_machine(self):
        self.assertTrue(any("expected arm" in p for p in check(image(), "arm")))


@unittest.skipUnless(sys.platform == "linux" and os.uname().machine == "x86_64",
                     "real images are linked for the Linux x86-64 host")
class RealImages(unittest.TestCase):
    """The loader's own fixture, linked by the host toolchain with the options that break
    a load: the checker must agree with what the linker produced."""

    # Images go under target/ (never the system temporary directory) and are overwritten by
    # the next run; the tests delete nothing.
    directory = Path(__file__).resolve().parents[2] / "target/sdk/check-elf-tests"

    @classmethod
    def setUpClass(cls):
        cls.directory.mkdir(parents=True, exist_ok=True)

    def link(self, name, flags):
        if shutil.which(os.environ.get("CC", "cc")) is None:
            self.skipTest("no C compiler")
        output = self.directory / name
        result = subprocess.run(["sh", FIXTURES / "build.sh", output], capture_output=True, text=True,
                                env={**os.environ, "XILA_FIXTURE_FLAGS": flags})
        if result.returncode != 0:
            self.skipTest(f"the host toolchain cannot link this image: {result.stderr.strip()[-200:]}")
        return output.read_bytes()

    def test_accepts_the_plain_image(self):
        self.assertEqual(check_elf.check(self.link("plain.elf", ""), "x86_64"), [])

    def test_rejects_packed_relative_relocations(self):
        problems = check_elf.check(self.link("relr.elf", "-Wl,-z,pack-relative-relocs"), "x86_64")
        self.assertTrue(any("pack-relative-relocs" in p for p in problems), problems)

    def test_rejects_a_constructor(self):
        problems = check_elf.check(self.link("constructor.elf", "-DXILA_FIXTURE_CONSTRUCTOR"), "x86_64")
        self.assertTrue(any("constructor" in p for p in problems), problems)


if __name__ == "__main__":
    unittest.main()
