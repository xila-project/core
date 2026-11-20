"""The POSIX layer's status-to-errno table agrees with Core's I/O error catalog."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CATALOG = ROOT / "libraries/io_error/src/catalog.rs"
HEADER = ROOT / "libraries/abi/application/include/xila/application.h"
TABLE = ROOT / "libraries/abi/application/sdk/posix/status.c"


class StatusTable(unittest.TestCase):
    def test_every_catalog_kind_maps_to_the_errno_the_catalog_projects(self):
        catalog = re.findall(r"(\w+) = (0x[0-9A-Fa-f]+) => (\w+),", CATALOG.read_text())
        self.assertGreater(len(catalog), 50)
        names = {int(value, 0): name for name, value in
                 re.findall(r"(XILA_STATUS_\w+)\s*=\s*(\w+)", HEADER.read_text())}
        table = dict(re.findall(r"\{(XILA_STATUS_\w+), (\w+)\}", TABLE.read_text()))
        for kind, identifier, errno in catalog:
            status = names.get(int(identifier, 16))
            self.assertIsNotNone(status, f"{kind} ({identifier}) has no XILA_STATUS_ value")
            self.assertEqual(table.get(status), errno, f"{kind}: {status}")


if __name__ == "__main__":
    unittest.main()
