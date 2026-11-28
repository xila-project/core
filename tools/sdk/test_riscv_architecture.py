import unittest
from pathlib import Path
from unittest.mock import patch

import build


class VerifyRiscvArchitecture(unittest.TestCase):
    SOFT = "riscv32imac-unknown-none-elf"
    SINGLE = "riscv32imafc-unknown-none-elf"

    def verify(self, output, target):
        image = Path("application.elf")
        with patch.object(build, "find_target_tool", return_value="llvm-readelf") as find_tool, \
                patch.object(build.subprocess, "check_output", return_value=output) as read:
            build.verify_riscv_architecture(image, target)
            find_tool.assert_called_once_with(target, "readelf")
            read.assert_called_once_with(["llvm-readelf", "--arch-specific", image], text=True)

    def test_accepts_firmware_extensions(self):
        self.verify("Value: rv32i2p1_m2p0_a2p1_c2p0_zmmul1p0_zaamo1p0_zalrsc1p0_zca1p0"
                    "_zicsr2p0_zifencei2p0", self.SOFT)
        self.verify("Value: rv32i2p1_f2p2_c2p0_zcf1p0", self.SINGLE)

    def test_rejects_extensions_absent_from_the_firmware(self):
        for target, architecture, extension in [
            (self.SOFT, "rv32i2p1_f2p2", "f"),
            (self.SINGLE, "rv32i2p1_d2p2", "d"),
        ]:
            with self.subTest(target=target), self.assertRaisesRegex(SystemExit, f"firmware lacks {extension}"):
                self.verify(f"Value: {architecture}", target)

    def test_rejects_missing_or_malformed_architecture(self):
        with self.assertRaisesRegex(SystemExit, "missing RISC-V architecture attribute"):
            self.verify("Value: 16", self.SOFT)
        with self.assertRaisesRegex(SystemExit, "invalid RISC-V extension"):
            self.verify("Value: rv32i2p1_invalid", self.SOFT)
