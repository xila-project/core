/* Compact RAM/PSRAM static PIE. Define XILA_DATA_ALIGN for page/MPU isolation. */
ENTRY(_xila_start)

PHDRS
{
  text    PT_LOAD FLAGS(5);
  data    PT_LOAD FLAGS(6);
  dynamic PT_DYNAMIC FLAGS(6);
  abi     PT_NOTE FLAGS(4);
}

SECTIONS
{
  . = 0;
  /* Xtensa L32R literal pools must precede their instructions. */
  .text : ALIGN(16) {
    *(.literal .literal.*)
    KEEP(*(.text._xila_start))
    *(.text .text.*)
  } :text
  .rodata : { *(.rodata .rodata.* .srodata .srodata.*) } :text
  .got.loc : { *(.got.loc) } :text
  /* Keep optional linker notes, including 8-byte-aligned GNU properties,
     outside the ABI note segment. */
  .note.gnu.build-id : { *(.note.gnu.build-id) } :text
  .note.gnu.property : { *(.note.gnu.property) } :text
  .note.xila.abi : ALIGN(4) { KEEP(*(.note.xila.abi)) } :text :abi
  .rel.dyn : { *(.rel.dyn) } :text
  .rela.dyn : { *(.rela.dyn) } :text
  /* .dynsym resets PHDR inheritance even when there are no relocations. */
  .dynsym : { *(.dynsym) } :text
  .gnu.hash : { *(.gnu.hash) } :text
  .hash : { *(.hash) } :text
  .dynstr : { *(.dynstr) } :text

  PROVIDE(XILA_DATA_ALIGN = 16);
  . = ALIGN(XILA_DATA_ALIGN);
  .dynamic : { *(.dynamic) } :data :dynamic
  .data : { *(.data .data.* .sdata .sdata.*) } :data
  .got : { *(.got .got.*) } :data
  .init_array : {
    PROVIDE_HIDDEN(__init_array_start = .);
    KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
    KEEP(*(.init_array))
    PROVIDE_HIDDEN(__init_array_end = .);
  } :data
  .bss (NOLOAD) : { *(.bss .bss.* .sbss .sbss.* COMMON) } :data

  /DISCARD/ : {
    *(.comment .note.GNU-stack)
    *(.xt.lit .xt.prop .xt.insn)
    *(.ARM.exidx* .ARM.extab* .eh_frame*)
  }
}
