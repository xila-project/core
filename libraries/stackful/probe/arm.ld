ENTRY(boot)
MEMORY {
  ROM (rx) : ORIGIN = 0x00000000, LENGTH = 1M
  RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 64K
}
_stack_top = ORIGIN(RAM) + LENGTH(RAM);
SECTIONS {
  .vectors : { KEEP(*(.vectors)) } > ROM
  .text : { *(.text .text.*) } > ROM
  .rodata : { *(.rodata .rodata.*) } > ROM
  .ARM.exidx : { *(.ARM.exidx*) } > ROM
  .data : { *(.data .data.*) } > RAM
  .bss : { *(.bss .bss.*) } > RAM
}
