ENTRY(_head)
SECTIONS
{
	. = 0;
	.text : { KEEP(*(.text.head)) *(.text*) }
	.rodata : { *(.rodata*) }
	.data.rel.ro : { *(.data.rel.ro*) }
	.got : { *(.got*) }
	.dynamic : { *(.dynamic) }
	.dynsym : { *(.dynsym) }
	.dynstr : { *(.dynstr) }
	.hash : { *(.hash) }
	.gnu.hash : { *(.gnu.hash) }
	.data : { *(.data*) }
	. = ALIGN(8);
	.rela.dyn : { __rela_start = .; *(.rela*) __rela_end = .; }
	. = ALIGN(16);
	_bss_start = .;
	.bss : { *(.bss*) *(COMMON) }
	. = ALIGN(16);
	_bss_end = .;
	. += 0x8000;
	. = ALIGN(16);
	_stack_top = .;
	. = ALIGN(4096);
	_end = .;
	/DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) *(.interp) }
}
