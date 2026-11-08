/* Rustland R++ runs flat at 4 MB under the PM stub; the stack follows the
   BSS, and the heap runs from there to 12 MB (programs it runs load at 12 MB). */
ENTRY(_start)
SECTIONS {
  . = 0x400000;
  .text : { KEEP(*(.text._start)) *(.text .text.*) }
  .rodata : { *(.rodata .rodata.*) }
  .data : { *(.data .data.*) *(.got .got.*) }
  .bss : {
    *(.bss .bss.*) *(COMMON)
    . = ALIGN(16);
    . += 0x100000;
  }
  /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment) *(.gnu*) *(.dynamic) *(.interp) }
}
