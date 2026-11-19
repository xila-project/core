"""The ELF checker rejects what the native loader rejects, using synthetic images."""

import struct
import unittest

import check_elf

PT_LOAD = 1
EM_X86_64, EM_RISCV = 62, 243
R_X86_64_RELATIVE, R_X86_64_64 = 8, 1
R_RISCV_RELATIVE = 3


def note(name=b"XILA\0", kind=1, descriptor=struct.pack("<I", 1)):
    padded = name + b"\0" * (-len(name) % 4)
    return struct.pack("<III", len(name), len(descriptor), kind) + padded + descriptor


def image(machine=EM_X86_64, etype=3, extra_segments=(), dynamic=(), relocations=(), notes=None):
    """A 64-bit little-endian ELF with the given program headers and one RELA section."""
    note_bytes = note() if notes is None else notes
    dynamic_bytes = b"".join(struct.pack("<qQ", tag, value) for tag, value in dynamic)
    dynamic_bytes += struct.pack("<qQ", 0, 0)
    rela = b"".join(struct.pack("<QQq", 0x1000 + 8 * index, (symbol << 32) | kind, 0)
                    for index, (kind, symbol) in enumerate(relocations))
    segments = [(2, len(dynamic_bytes)), (4, len(note_bytes)), *[(kind, 0) for kind in extra_segments]]
    header_size = 64 + 56 * len(segments)
    offsets, cursor = [], header_size
    for payload in (dynamic_bytes, note_bytes):
        offsets.append(cursor)
        cursor += len(payload)
    rela_offset = cursor
    section_offset = rela_offset + len(rela)
    body = dynamic_bytes + note_bytes + rela
    phdrs = b""
    for index, (kind, size) in enumerate(segments):
        offset = offsets[index] if index < 2 else 0
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
        self.assertEqual(check_elf.check(data, "x86_64"), [])

    def test_rejects_a_fixed_executable(self):
        self.assertTrue(any("ET_DYN" in p for p in check_elf.check(image(etype=2))))

    def test_rejects_an_interpreter_and_tls(self):
        problems = check_elf.check(image(extra_segments=[check_elf.PT_INTERP, check_elf.PT_TLS]))
        self.assertTrue(any("PT_INTERP" in p for p in problems))
        self.assertTrue(any("PT_TLS" in p for p in problems))

    def test_rejects_a_missing_abi_note(self):
        problems = check_elf.check(image(notes=note(name=b"GNU\0")))
        self.assertTrue(any("XILA" in p for p in problems))

    def test_rejects_symbol_relocations(self):
        problems = check_elf.check(image(relocations=[(R_X86_64_64, 3)]))
        self.assertTrue(any("not RELATIVE" in p for p in problems))
        problems = check_elf.check(image(relocations=[(R_X86_64_RELATIVE, 3)]))
        self.assertTrue(any("symbol" in p for p in problems))

    def test_rejects_constructors_and_dependencies(self):
        for tag in (check_elf.DT_NEEDED, check_elf.DT_INIT, check_elf.DT_FINI):
            self.assertTrue(check_elf.check(image(dynamic=[(tag, 1)])), tag)
        problems = check_elf.check(image(dynamic=[(check_elf.DT_INIT_ARRAYSZ, 8)]))
        self.assertTrue(any("init/fini" in p for p in problems))

    def test_rejects_riscv_linker_relaxation(self):
        good = image(machine=EM_RISCV, relocations=[(R_RISCV_RELATIVE, 0)])
        self.assertEqual(check_elf.check(good, "riscv32"), [])
        bad = image(machine=EM_RISCV, relocations=[(check_elf.R_RISCV_RELAX, 0)])
        self.assertTrue(any("R_RISCV_RELAX" in p for p in check_elf.check(bad, "riscv32")))

    def test_rejects_the_wrong_machine(self):
        self.assertTrue(any("expected arm" in p for p in check_elf.check(image(), "arm")))


if __name__ == "__main__":
    unittest.main()
