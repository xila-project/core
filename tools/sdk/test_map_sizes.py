"""build.py's per-origin size table reads lld maps by column, not by name."""

import unittest

import build

HEADER = "     VMA      LMA     Size Align Out     In      Symbol\n"
SDK = "/w/target/sdk/{t}/xila-sdk-0.1-{t}/lib"

# RISC-V: `.Lpcrel_hiN` labels start with a dot and sit in the Symbol column. Taking them for
# output sections credited every later input section to a section that does not exist.
RISCV = HEADER + f"""       0        0        0     1 
       0        0        0     1 . = 0
       0        0     41b0    16 .text
       0        0       50     2         {SDK.format(t="riscv32imac")}/crt0.o:(.text._xila_start)
       0        0        0     1                 $x
       0        0       50     1                 _xila_start
      28       28        0     1                 .Lpcrel_hi0
      50       50      a10     2         CMakeFiles/hello.dir/hello.c.obj:(.text.main)
      50       50        0     1                 $x
      50       50      a10     1                 main
      6c       6c        0     1                 .Lpcrel_hi0
      8e       8e        0     1                 .Lpcrel_hi4
     a60      a60      1d4     2         {SDK.format(t="riscv32imac")}/libc.a(libc_stdio_printf.o):(.text.printf)
     a60      a60      1d4     1                 printf
     a70      a70        0     1                 .Lpcrel_hi2
    4200     4200       40     8 .rodata
    4200     4200       30     4         CMakeFiles/hello.dir/hello.c.obj:(.rodata.str1.1)
    4230     4230       10     4         {SDK.format(t="riscv32imac")}/libc.a(libc_string_strlen.o):(.rodata)
    4240     4240       14     8 .data
    4240     4240       14     4         {SDK.format(t="riscv32imac")}/libxila.a(stubs.o):(.data.xila_api)
"""

THUMB = HEADER + f"""       0        0        0     1 
       0        0     33b8    16 .text
       0        0       40     4         {SDK.format(t="thumbv7em")}/crt0.o:(.text._xila_start)
       0        0        0     1                 $t
       1        1       40     1                 _xila_start
      38       38        0     1                 $d
      40       40      908     4         CMakeFiles/hello.dir/hello.c.obj:(.text.main)
      40       40        0     1                 $t
      41       41      908     1                 main
     948      948       24     4         {SDK.format(t="thumbv7em")}/libxila_posix.a(errno.o):(.text.__xila_errno_location)
     948      948        0     1                 $t
     949      949       24     1                 __xila_errno_location
    3400     3400       20     4 .rodata
    3400     3400       20     4         CMakeFiles/hello.dir/hello.c.obj:(.rodata.str1.1)
    3420     3420       10     4 .bss
    3420     3420       10     4         {SDK.format(t="thumbv7em")}/libxila_posix.a(fd.o):(.bss.table)
"""


class MapSizes(unittest.TestCase):
    def test_riscv_local_labels_are_not_output_sections(self):
        self.assertEqual(build.map_sizes(RISCV), {
            "crt0.o": {".text": 0x50},
            "application": {".text": 0xA10, ".rodata": 0x30},
            "libc.a": {".text": 0x1D4, ".rodata": 0x10},
            "libxila.a": {".data": 0x14},
        })

    def test_thumb_mapping_symbols_are_not_output_sections(self):
        self.assertEqual(build.map_sizes(THUMB), {
            "crt0.o": {".text": 0x40},
            "application": {".text": 0x908, ".rodata": 0x20},
            "libxila_posix.a": {".text": 0x24, ".bss": 0x10},
        })

    def test_an_excerpt_without_a_header_reads_the_same(self):
        body = RISCV.removeprefix(HEADER)
        self.assertEqual(build.map_sizes(body), build.map_sizes(RISCV))


if __name__ == "__main__":
    unittest.main()
