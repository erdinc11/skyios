use crate::console::{self, FB_BASE, FB_H, FB_STRIDE, FB_W};
use crate::fdt;
use core::mem::offset_of;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

const BOOT_LINE_LENGTH: usize = 608;
const PAGE_16K: u64 = 0x4000;

#[repr(C)]
struct PayloadSeg {
    vmaddr: u64,
    vmsize: u64,
    file_off: u64,
    file_size: u64,
}

#[repr(C)]
struct Payload {
    magic: [u8; 8],
    version: u32,
    nsegs: u32,
    virt_base: u64,
    entry_va: u64,
    adt_off: u64,
    adt_size: u64,
    kernel_end: u64,
    phys_base: u64,
    mem_size: u64,
    cmdline: [u8; BOOT_LINE_LENGTH],
}

#[repr(C)]
struct BootVideo {
    base: u64,
    display: u64,
    row_bytes: u64,
    width: u64,
    height: u64,
    depth: u64,
}

#[repr(C)]
struct BootArgs {
    revision: u16,
    version: u16,
    virt_base: u64,
    phys_base: u64,
    mem_size: u64,
    top_of_kernel_data: u64,
    video: BootVideo,
    machine_type: u32,
    device_tree: u64,
    device_tree_length: u32,
    command_line: [u8; BOOT_LINE_LENGTH],
    boot_flags: u64,
    mem_size_actual: u64,
}

const _: () = assert!(size_of::<Payload>() == 0x2a8);
const _: () = assert!(offset_of!(BootArgs, video) == 40);
const _: () = assert!(offset_of!(BootArgs, command_line) == 108);
const _: () = assert!(size_of::<BootArgs>() == 736);

fn copy64(dst: u64, src: u64, n: u64) {
    for i in 0..n.div_ceil(8) {
        unsafe {
            let v = read_volatile((src as *const u64).add(i as usize));
            write_volatile((dst as *mut u64).add(i as usize), v);
        }
    }
}

fn zero64(dst: u64, n: u64) {
    for i in 0..n / 8 {
        unsafe { write_volatile((dst as *mut u64).add(i as usize), 0) };
    }
}

macro_rules! rd {
    ($p:expr, $f:ident) => {
        unsafe { read_volatile(addr_of!((*$p).$f)) }
    };
}

macro_rules! wr {
    ($p:expr, $f:ident, $v:expr) => {
        unsafe { write_volatile(addr_of_mut!((*$p).$f), $v) }
    };
}

pub fn boot(dtb: usize) -> &'static [u8] {
    let (Some(rd_start), Some(rd_end)) =
        (fdt::chosen_u64(dtb, b"linux,initrd-start"), fdt::chosen_u64(dtb, b"linux,initrd-end"))
    else {
        return b"no initrd in /chosen";
    };
    console::hex2(b"INITRD", b"lo=", rd_start as u32, Some((b"hi=", rd_end as u32)));

    let p = rd_start as *const Payload;
    if rd!(p, magic) != *b"SWEETXNU" {
        return b"no SWEETXNU payload";
    }
    let (adt_off, adt_size) = (rd!(p, adt_off), rd!(p, adt_size));
    if rd!(p, version) != 1 || adt_off & 7 != 0 || rd_start & 7 != 0 {
        return b"bad payload header";
    }
    let (phys_base, virt_base, mem_size) = (rd!(p, phys_base), rd!(p, virt_base), rd!(p, mem_size));
    let pa_of_va = phys_base.wrapping_sub(virt_base);
    console::hex2(b"PHYS", b"base=", phys_base as u32, Some((b"size=", mem_size as u32)));
    console::hex64(b"VIRT", virt_base);
    console::hex64(b"ENTRY", rd!(p, entry_va));

    if rd_end > phys_base && rd_start < phys_base + mem_size {
        return b"ramdisk overlaps kernel memory";
    }

    let nsegs = rd!(p, nsegs);
    let segs = (rd_start + size_of::<Payload>() as u64) as *const PayloadSeg;
    for i in 0..nsegs as usize {
        let s = unsafe { segs.add(i) };
        let (vmaddr, vmsize, file_off, file_size) = (rd!(s, vmaddr), rd!(s, vmsize), rd!(s, file_off), rd!(s, file_size));
        let dst = vmaddr.wrapping_add(pa_of_va);
        copy64(dst, rd_start + file_off, file_size);
        let filled = (file_size + 7) & !7;
        if vmsize > file_size {
            zero64(dst.wrapping_add(filled), vmsize.wrapping_sub(filled));
        }
    }
    let kernel_end = rd!(p, kernel_end);
    console::hex2(b"SEGS", b"n=", nsegs, Some((b"end=", kernel_end.wrapping_add(pa_of_va) as u32)));

    let adt_pa = kernel_end.wrapping_add(pa_of_va);
    copy64(adt_pa, rd_start + adt_off, adt_size);
    let ba_pa = (adt_pa + adt_size + PAGE_16K - 1) & !(PAGE_16K - 1);
    let top = ba_pa + PAGE_16K;

    zero64(ba_pa, PAGE_16K);
    let ba = ba_pa as *mut BootArgs;
    wr!(ba, revision, 2);
    wr!(ba, version, 2);
    wr!(ba, virt_base, virt_base);
    wr!(ba, phys_base, phys_base);
    wr!(ba, mem_size, mem_size);
    wr!(ba, top_of_kernel_data, top);
    let v = unsafe { addr_of_mut!((*ba).video) };
    wr!(v, base, FB_BASE);
    wr!(v, display, 1);
    wr!(v, row_bytes, FB_STRIDE as u64);
    wr!(v, width, FB_W as u64);
    wr!(v, height, FB_H as u64);
    wr!(v, depth, 32);
    wr!(ba, device_tree, adt_pa.wrapping_sub(pa_of_va));
    wr!(ba, device_tree_length, adt_size as u32);
    let src = unsafe { addr_of!((*p).cmdline) } as *const u8;
    let dst = unsafe { addr_of_mut!((*ba).command_line) } as *mut u8;
    for i in 0..BOOT_LINE_LENGTH {
        unsafe { write_volatile(dst.add(i), read_volatile(src.add(i))) };
    }
    wr!(ba, mem_size_actual, mem_size);
    console::hex2(b"BOOTARG", b"pa=", ba_pa as u32, Some((b"top=", top as u32)));

    let entry_pa = rd!(p, entry_va).wrapping_add(pa_of_va);
    console::hex64(b"JUMP", entry_pa);
    console::str(b"XNU", b"jumping (fbcon clears screen)");

    unsafe {
        core::arch::asm!("dsb sy", "ic iallu", "dsb sy", "isb", options(nostack));
        let entry: extern "C" fn(u64) -> ! = core::mem::transmute(entry_pa as usize);
        entry(ba_pa)
    }
}
