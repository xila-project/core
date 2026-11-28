/* Split-bus static PIE for Harvard targets (ESP32-S2 / ESP32-S3 Xtensa):
   `:text` holds only instructions and 32-bit aligned `.literal` pools so it can
   execute from the IRAM alias, while `:rodata` and `:data` carry no PF_X flag
   so the loader relocates all byte-addressed data references to the DRAM alias. */
ENTRY(_xila_start)

PHDRS
{
  text    PT_LOAD FLAGS(5);
  rodata  PT_LOAD FLAGS(4);
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
  .rodata : ALIGN(16) { *(.rodata .rodata.* .srodata .srodata.*) } :rodata
  .got.loc : { *(.got.loc) } :rodata
  /* Keep optional linker notes, including 8-byte-aligned GNU properties,
     outside the ABI note segment. */
  .note.gnu.build-id : { *(.note.gnu.build-id) } :rodata
  .note.gnu.property : { *(.note.gnu.property) } :rodata
  .note.xila.abi : ALIGN(4) { KEEP(*(.note.xila.abi)) } :rodata :abi
  /* Any other note (.note.package, custom notes) would otherwise follow the ABI
     note and inherit its segments; the explicit :rodata keeps it out of :abi. */
  .note.other : { *(.note .note.*) } :rodata
  .rel.dyn : { *(.rel.dyn) } :rodata
  .rela.dyn : { *(.rela.dyn) } :rodata
  .dynsym : { *(.dynsym) } :rodata
  .gnu.hash : { *(.gnu.hash) } :rodata
  .hash : { *(.hash) } :rodata
  .dynstr : { *(.dynstr) } :rodata

  PROVIDE(XILA_DATA_ALIGN = 16);
  . = ALIGN(XILA_DATA_ALIGN);
  .dynamic : { *(.dynamic) } :data :dynamic
  .data : { *(.data .data.* .sdata .sdata.*) } :data
  .got : { *(.got .got.*) } :data
  .preinit_array : {
    KEEP(*(SORT_BY_INIT_PRIORITY(.preinit_array.*)))
    KEEP(*(.preinit_array))
  } :data
  .init_array : {
    PROVIDE_HIDDEN(__init_array_start = .);
    KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
    KEEP(*(.init_array))
    PROVIDE_HIDDEN(__init_array_end = .);
  } :data
  .fini_array : {
    KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*)))
    KEEP(*(.fini_array))
  } :data
  .bss (NOLOAD) : { *(.bss .bss.* .sbss .sbss.* COMMON) } :data

  /DISCARD/ : {
    *(.comment .note.GNU-stack)
    *(.xt.lit .xt.prop .xt.insn)
    *(.ARM.exidx* .ARM.extab* .eh_frame*)
  }
}
