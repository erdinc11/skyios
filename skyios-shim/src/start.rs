core::arch::global_asm!(
    r#"
	.section .text.head, "ax"
	.globl _head
_head:
	b	_start
	.long	0
	.quad	0
	.quad	_end - _head
	.quad	0xa
	.quad	0
	.quad	0
	.quad	0
	.ascii	"ARM\x64"
	.long	0

	.text
	.globl _start
_start:
	msr	daifset, #0xf
	mov	x19, x0
	msr	spsel, #1
	adr	x1, _stack_top
	mov	sp, x1
	adr	x9, _head
	adr	x10, __rela_start
	adr	x11, __rela_end
1:	cmp	x10, x11
	b.hs	2f
	ldp	x12, x13, [x10], #16
	ldr	x14, [x10], #8
	cmp	w13, #1027
	b.ne	1b
	add	x14, x14, x9
	str	x14, [x9, x12]
	b	1b
2:	adr	x1, vectors
	msr	vbar_el1, x1
	isb
	adr	x1, _bss_start
	adr	x2, _bss_end
3:	cmp	x1, x2
	b.hs	4f
	str	xzr, [x1], #8
	b	3b
4:	mov	x0, x19
	bl	shim_main
5:	wfe
	b	5b

.macro vent slot
	.balign	0x80
	stp	x29, x30, [sp, #-16]!
	mov	x29, #\slot
	b	exc_common
.endm

	.balign	0x800
	.globl vectors
vectors:
	vent 0
	vent 1
	vent 2
	vent 3
	vent 4
	vent 5
	vent 6
	vent 7
	vent 8
	vent 9
	vent 10
	vent 11
	vent 12
	vent 13
	vent 14
	vent 15

exc_common:
	sub	sp, sp, #(30 * 8)
	stp	x0, x1, [sp, #0]
	stp	x2, x3, [sp, #16]
	stp	x4, x5, [sp, #32]
	stp	x6, x7, [sp, #48]
	stp	x8, x9, [sp, #64]
	stp	x10, x11, [sp, #80]
	stp	x12, x13, [sp, #96]
	stp	x14, x15, [sp, #112]
	stp	x16, x17, [sp, #128]
	stp	x18, x19, [sp, #144]
	stp	x20, x21, [sp, #160]
	stp	x22, x23, [sp, #176]
	stp	x24, x25, [sp, #192]
	stp	x26, x27, [sp, #208]
	str	x28, [sp, #224]
	mov	x0, x29
	mov	x1, sp
	bl	exc_handler
6:	wfe
	b	6b
"#
);
