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
EM_ARM, EM_X86_64, EM_XTENSA, EM_RISCV = 40, 62, 94, 243
R_X86_64_RELATIVE, R_X86_64_64 = 8, 1
R_RISCV_RELATIVE = 3
R_ARM_RELATIVE, R_XTENSA_RELATIVE = 23, 5
HOST_ABI = (0, 1)
VERSION = struct.pack("<I", (0 << 16) | 1)
FIXTURES = Path(__file__).resolve().parents[2] / "libraries/native_loader/fixtures"


def check(data, machine=None, abi=HOST_ABI, **target):
    """check_elf.check against a fixed host ABI, so the tests do not move with the header."""
    return check_elf.check(data, machine, abi, **target)


def note(name=b"XILA\0", kind=1, descriptor=VERSION):
    padded = name + b"\0" * (-len(name) % 4)
    return struct.pack("<III", len(name), len(descriptor), kind) + padded + descriptor


def image(machine=EM_X86_64, etype=3, extra_segments=(), dynamic=(), relocations=(), notes=None,
          second_notes=None, terminate=True, wide=True, flags=0, entry=None, load_flags=5,
          load_align=8, load_count=1, dynamic_tail=0, symbols=()):
    """A little-endian ELF (64-bit, or 32-bit with `wide=False`) with one PT_LOAD over the
    whole file, the given program headers and one relocation section. `second_notes` adds a
    second PT_NOTE segment; `terminate=False` omits DT_NULL; `dynamic_tail` appends that many
    stray bytes to PT_DYNAMIC. `entry` defaults to an address inside the PT_LOAD (with the
    Thumb bit on Arm). `symbols` is a list of (name, defined) added as a symbol table."""
    word, ehsize, phsize = (8, 64, 56) if wide else (4, 52, 32)
    note_bytes = note() if notes is None else notes
    dynamic_bytes = b"".join(struct.pack("<qQ" if wide else "<iI", tag, value) for tag, value in dynamic)
    if terminate:
        dynamic_bytes += struct.pack("<qQ" if wide else "<iI", 0, 0)
    dynamic_bytes += bytes(dynamic_tail)
    if wide:
        rela = b"".join(struct.pack("<QQq", 0x1000 + 8 * index, (symbol << 32) | kind, 0)
                        for index, (kind, symbol) in enumerate(relocations))
    else:
        rela = b"".join(struct.pack("<IIi", 0x1000 + 4 * index, (symbol << 8) | kind, 0)
                        for index, (kind, symbol) in enumerate(relocations))
    payloads = [dynamic_bytes, note_bytes]
    kinds = [2, 4]
    if second_notes is not None:
        payloads.append(second_notes)
        kinds.append(4)
    segments = [(kind, len(payload)) for kind, payload in zip(kinds, payloads)]
    segments += [(kind, 0) for kind in extra_segments]
    count = load_count + len(segments)
    header_size = ehsize + phsize * count
    offsets, cursor = [], header_size
    for payload in payloads:
        offsets.append(cursor)
        cursor += len(payload)
    rela_offset = cursor
    strings, table = b"\0", b"\0" * (24 if wide else 16)
    for name, defined in symbols:
        index, strings = len(strings), strings + name + b"\0"
        table += (struct.pack("<IBBHQQ", index, 0, 0, 1 if defined else 0, 0, 0) if wide
                  else struct.pack("<IIIBBH", index, 0, 0, 0, 0, 1 if defined else 0))
    symbol_offset = rela_offset + len(rela)
    section_offset = symbol_offset + (len(table) + len(strings) if symbols else 0)
    body = b"".join(payloads) + rela + (table + strings if symbols else b"")
    phdrs = b""
    for _ in range(load_count):
        if wide:
            phdrs += struct.pack("<IIQQQQQQ", PT_LOAD, load_flags, 0, 0, 0, section_offset,
                                 section_offset, load_align)
        else:
            phdrs += struct.pack("<IIIIIIII", PT_LOAD, 0, 0, 0, section_offset, section_offset,
                                 load_flags, load_align)
    for index, (kind, size) in enumerate(segments):
        offset = offsets[index] if index < len(payloads) else 0
        if wide:
            phdrs += struct.pack("<IIQQQQQQ", kind, 4, offset, offset, offset, size, size, 8)
        else:
            phdrs += struct.pack("<IIIIIIII", kind, offset, offset, offset, size, size, 4, 8)
    shentsize = 64 if wide else 40
    if wide:
        sections = struct.pack("<IIQQQQIIQQ", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
        sections += struct.pack("<IIQQQQIIQQ", 0, check_elf.SHT_RELA, 0, 0, rela_offset, len(rela),
                                0, 0, 8, 24)
        if symbols:
            sections += struct.pack("<IIQQQQIIQQ", 0, check_elf.SHT_SYMTAB, 0, 0, symbol_offset,
                                    len(table), 3, 0, 8, 24)
            sections += struct.pack("<IIQQQQIIQQ", 0, check_elf.SHT_STRTAB, 0, 0,
                                    symbol_offset + len(table), len(strings), 0, 0, 1, 0)
    else:
        sections = struct.pack("<IIIIIIIIII", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
        sections += struct.pack("<IIIIIIIIII", 0, check_elf.SHT_RELA, 0, 0, rela_offset, len(rela),
                                0, 0, 4, 12)
        if symbols:
            sections += struct.pack("<IIIIIIIIII", 0, check_elf.SHT_SYMTAB, 0, 0, symbol_offset,
                                    len(table), 3, 0, 4, 16)
            sections += struct.pack("<IIIIIIIIII", 0, check_elf.SHT_STRTAB, 0, 0,
                                    symbol_offset + len(table), len(strings), 0, 0, 1, 0)
    if entry is None:
        entry = header_size | (1 if machine == EM_ARM else 0)
    header = b"\x7fELF" + bytes([2 if wide else 1, 1, 1, 0]) + bytes(8)
    header += struct.pack("<HHIQQQIHHHHHH" if wide else "<HHIIIIIHHHHHH", etype, machine, 1, entry,
                          ehsize, section_offset, flags, ehsize, phsize, count, shentsize,
                          4 if symbols else 2, 0)
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

    def test_rejects_riscv_gp_relaxation(self):
        # Without a gp symbol lld has nothing to relax against; with one the image may use
        # a gp the firmware does not share. A local label or an undefined reference is fine.
        good = image(machine=EM_RISCV, wide=False, relocations=[(R_RISCV_RELATIVE, 0)],
                     symbols=[(b"main", True), (b"__global_pointer$", False)])
        self.assertEqual(check(good, "riscv32"), [])
        bad = image(machine=EM_RISCV, wide=False, relocations=[(R_RISCV_RELATIVE, 0)],
                    symbols=[(b"main", True), (b"__global_pointer$", True)])
        self.assertTrue(any("__global_pointer$" in p for p in check(bad, "riscv32")))
        # The symbol means nothing on other architectures, and a 64-bit table parses too.
        wide = image(machine=EM_RISCV, relocations=[(R_RISCV_RELATIVE, 0)],
                     symbols=[(b"__global_pointer$", True)])
        self.assertTrue(any("__global_pointer$" in p for p in check(wide, "riscv64")))

    def test_rejects_riscv_relax_relocations_as_non_relative(self):
        # R_RISCV_RELAX (51) is an object-file marker: in an image it is not RELATIVE.
        bad = image(machine=EM_RISCV, wide=False, relocations=[(51, 0)])
        self.assertTrue(any("not RELATIVE" in p for p in check(bad, "riscv32")))

    def test_rejects_the_wrong_machine(self):
        self.assertTrue(any("expected arm" in p for p in check(image(), "arm")))


class LoaderRules(unittest.TestCase):
    """The header, segment, entry, dynamic-table and `e_flags` rules of Elf::parse."""

    def problems(self, data, text, **target):
        found = check(data, **target)
        self.assertTrue(any(text in problem for problem in found), (text, found))

    def test_accepts_a_thumb_image_for_the_hard_float_target(self):
        data = image(machine=EM_ARM, wide=False, flags=0x05000400,
                     relocations=[(R_ARM_RELATIVE, 0)] * 2)
        self.assertEqual(check(data, "arm", float_abi="hard"), [])
        self.assertEqual(check(data, "arm"), [])

    def test_arm_flags_follow_the_loader(self):
        def arm(flags, **target):
            return check(image(machine=EM_ARM, wide=False, flags=flags), "arm", **target)
        # EABI 5 with the target's float ABI, or with no float flag at all.
        self.assertEqual(arm(0x05000400, float_abi="hard"), [])
        self.assertEqual(arm(0x05000000, float_abi="hard"), [])
        self.assertEqual(arm(0x05000200, float_abi="soft"), [])
        self.assertEqual(arm(0x05000000, float_abi="soft"), [])
        # A soft-float image on the hard-float target, and the reverse.
        self.problems(image(machine=EM_ARM, wide=False, flags=0x05000200), "float ABI",
                      machine="arm", float_abi="hard")
        self.problems(image(machine=EM_ARM, wide=False, flags=0x05000400), "float ABI",
                      machine="arm", float_abi="soft")
        # Both float flags is never accepted; with no target given, one of them is.
        self.problems(image(machine=EM_ARM, wide=False, flags=0x05000600), "float ABI",
                      machine="arm")
        self.assertEqual(arm(0x05000400), [])
        self.assertEqual(arm(0x05000200), [])
        # Other EABI versions and big-endian code.
        self.problems(image(machine=EM_ARM, wide=False, flags=0x04000400), "EABI version 5",
                      machine="arm", float_abi="hard")
        self.problems(image(machine=EM_ARM, wide=False, flags=0x05800400), "BE8",
                      machine="arm", float_abi="hard")
        self.problems(image(machine=EM_ARM, wide=False, flags=0), "EABI version 5", machine="arm")

    def test_softfp_is_the_soft_variant_on_arm(self):
        def arm(flags, **target):
            return check(image(machine=EM_ARM, wide=False, flags=flags), "arm", **target)
        self.assertEqual(arm(0x05000200, float_abi="softfp"), [])
        self.assertEqual(arm(0x05000000, float_abi="softfp"), [])
        self.problems(image(machine=EM_ARM, wide=False, flags=0x05000400), "float ABI",
                      machine="arm", float_abi="softfp")

    def test_the_float_abi_must_exist_on_the_architecture(self):
        arm = image(machine=EM_ARM, wide=False, flags=0x05000400)
        riscv = image(machine=EM_RISCV, wide=False, flags=0x5)
        for data, machine, wrong in [
            (arm, "arm", "single"), (arm, "arm", "double"),
            (riscv, "riscv32", "hard"), (riscv, "riscv32", "softfp"),
            (image(), "x86_64", "hard"), (image(machine=EM_XTENSA, wide=False), "xtensa", "soft"),
        ]:
            with self.subTest(machine=machine, float_abi=wrong):
                with self.assertRaises(check_elf.ElfError) as raised:
                    check(data, machine, float_abi=wrong)
                self.assertIn(wrong, str(raised.exception))
        # The image decides when no machine is named.
        with self.assertRaises(check_elf.ElfError):
            check(riscv, float_abi="hard")
        # A wrong machine is the problem to report, not the float ABI.
        self.problems(arm, "machine is arm, expected riscv32", machine="riscv32", float_abi="double")

    def test_the_arm_entry_selects_thumb(self):
        data = image(machine=EM_ARM, wide=False, flags=0x05000400, entry=0x80)
        self.problems(data, "Thumb", machine="arm")

    def test_riscv32_rejects_elf64(self):
        self.problems(image(machine=EM_RISCV), "machine is riscv64, expected riscv32",
                      machine="riscv32", float_abi="soft")

    def test_riscv_flags_follow_the_loader(self):
        def riscv(flags, **target):
            return check(image(machine=EM_RISCV, wide=False, flags=flags), "riscv32", **target)
        # imac with the soft-float ABI: RVC set.
        self.assertEqual(riscv(0x1, float_abi="soft"), [])
        self.assertEqual(riscv(0x0, float_abi="soft"), [])
        self.assertEqual(riscv(0x5, float_abi="double"), [])
        self.assertEqual(riscv(0x3, float_abi="single"), [])
        self.problems(image(machine=EM_RISCV, wide=False, flags=0x3), "float ABI is not soft",
                      machine="riscv32", float_abi="soft")
        self.problems(image(machine=EM_RISCV, wide=False, flags=0x1), "compressed",
                      machine="riscv32", float_abi="soft", compressed=False)
        self.problems(image(machine=EM_RISCV, wide=False, flags=0x9), "RVE",
                      machine="riscv32", float_abi="soft")
        self.assertEqual(riscv(0x9, float_abi="soft", embedded=True), [])
        self.problems(image(machine=EM_RISCV, wide=False, flags=0x20), "unsupported RISC-V flag",
                      machine="riscv32", float_abi="soft")
        # TSO (0x10) is not accepted either.
        self.problems(image(machine=EM_RISCV, wide=False, flags=0x10), "unsupported RISC-V flag",
                      machine="riscv32", float_abi="soft")

    def test_x86_64_flags_must_be_zero_and_xtensa_flags_are_not_checked(self):
        self.problems(image(flags=1), "must be zero", machine="x86_64")
        data = image(machine=EM_XTENSA, wide=False, flags=0x12345, relocations=[(R_XTENSA_RELATIVE, 0)])
        self.assertEqual(check(data, "xtensa"), [])
        self.problems(image(machine=EM_XTENSA, wide=False, relocations=[(1, 0)]),
                      "not RELATIVE", machine="xtensa")

    def test_a_truncated_dynamic_entry_is_an_error_not_a_crash(self):
        data = image(dynamic=[(check_elf.DT_RELA, 0x40)])
        header = 64 + 56 * 4
        with self.assertRaises(check_elf.ElfError):
            check(data[: header + 20])
        # Cut inside an entry of a 32-bit image as well.
        small = image(machine=EM_ARM, wide=False, flags=0x05000400, dynamic=[(check_elf.DT_RELA, 0x40)])
        with self.assertRaises(check_elf.ElfError):
            check(small[: 52 + 32 * 4 + 12])
        # Too short to hold the header.
        with self.assertRaises(check_elf.ElfError):
            check(image()[:40])

    def test_a_truncated_header_is_an_error_not_a_crash(self):
        for data in (b"\x7fELF", b"\x7fELF\x02", b"\x7fELF\x02\x01", b"\x7fELF\x02\x01\x01",
                     image()[:63], image(machine=EM_ARM, wide=False)[:51]):
            with self.subTest(length=len(data)):
                with self.assertRaises(check_elf.ElfError):
                    check(data)
        with self.assertRaises(check_elf.ElfError):
            check(b"\x7fELF\x07\x01" + bytes(60))

    def test_dynamic_entries_must_be_whole(self):
        self.problems(image(dynamic_tail=4), "whole number of entries")

    def test_pt_load_alignment(self):
        for align in (3, 0x20000, 0x1_0000_0000):
            self.problems(image(load_align=align), "alignment")
        for align in (0, 1, 16, 0x10000):
            self.assertEqual(check(image(load_align=align)), [], align)

    def test_the_entry_is_inside_an_executable_load(self):
        self.problems(image(entry=1 << 20), "entry point")
        self.problems(image(load_flags=4), "entry point")  # readable only
        self.problems(image(load_count=0), "entry point")
        self.assertEqual(check(image(load_flags=7)), [])

    def test_the_dynamic_section_is_inside_a_load(self):
        self.problems(image(load_count=0), "PT_DYNAMIC is empty")

    def test_relocation_tables_are_complete_with_the_loaders_entry_size(self):
        tags = check_elf
        good = [(tags.DT_RELA, 0x40), (tags.DT_RELASZ, 48), (tags.DT_RELA_ENTRY, 24)]
        self.assertEqual(check(image(dynamic=good)), [])
        self.problems(image(dynamic=good[:2]), "DT_RELA table")
        self.problems(image(dynamic=good[:1] + good[2:]), "DT_RELA table")
        self.problems(image(dynamic=[good[0], good[1], (tags.DT_RELA_ENTRY, 16)]), "DT_RELA table")
        self.problems(image(dynamic=[good[0], (tags.DT_RELASZ, 40), good[2]]), "DT_RELA table")
        self.problems(image(dynamic=[(tags.DT_RELA, 1 << 20)] + good[1:]), "DT_RELA table")
        # DT_REL tables are two words per entry: 8 bytes on a 32-bit image, 16 on a 64-bit one.
        rel = [(tags.DT_REL, 0x40), (tags.DT_RELSZ, 16), (tags.DT_REL_ENTRY, 8)]
        arm = dict(machine=EM_ARM, wide=False, flags=0x05000400)
        self.assertEqual(check(image(dynamic=rel, **arm), "arm"), [])
        self.problems(image(dynamic=rel[:2] + [(tags.DT_REL_ENTRY, 16)], **arm), "DT_REL table",
                      machine="arm")
        # An empty table needs no bytes behind it.
        self.assertEqual(check(image(dynamic=[(tags.DT_RELA, 1 << 20), (tags.DT_RELASZ, 0),
                                              (tags.DT_RELA_ENTRY, 24)])), [])

    def test_program_header_counts(self):
        self.problems(image(extra_segments=[0x6474E551] * 16), "program headers")


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
