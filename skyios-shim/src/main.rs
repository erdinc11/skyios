#![no_std]
#![no_main]

mod arch;
mod console;
mod fdt;
mod font;
mod loader;
mod start;
mod ufs;

use arch::sysreg;

static mut STAGE: &[u8] = b"start";

fn stage(s: &'static [u8]) {
    unsafe { STAGE = s };
}

#[unsafe(no_mangle)]
pub extern "C" fn shim_main(dtb: u64) -> ! {
    stage(b"loader");
    console::clear(console::RED);
    console::str(b"SHIM", b"SkyIOS shim (Rust): xnu loader");
    console::skip();
    let why = loader::boot(dtb as usize);
    console::str(b"LOADER", why);
    console::skip();

    stage(b"ufs");
    ufs::probe();
    console::skip();
    console::str(b"UFS", b"probe done, reset in ~3 min");
    arch::delay_ms(180_000);
    arch::reset(20)
}

#[unsafe(no_mangle)]
pub extern "C" fn exc_handler(slot: u64, regs: *const u64) -> ! {
    console::clear(console::BLUE);
    console::str(
        b"PANIC",
        match slot & 3 {
            0 => b"SYNC",
            1 => b"IRQ",
            2 => b"FIQ",
            _ => b"SERROR",
        },
    );
    console::hex2(b"SLOT", b"n=", slot as u32, Some((b"cpu=", sysreg!("mpidr_el1") as u32)));
    console::str(b"STAGE", unsafe { STAGE });
    console::hex64(b"ESR", sysreg!("esr_el1"));
    console::hex64(b"ELR", sysreg!("elr_el1"));
    console::hex64(b"FAR", sysreg!("far_el1"));
    console::hex64(b"SPSR", sysreg!("spsr_el1"));
    unsafe {
        console::hex64(b"X0", *regs);
        console::hex64(b"X1", *regs.add(1));
        console::hex64(b"X30", *regs.add(29));
    }
    arch::reset(40)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    console::clear(console::BLUE);
    console::str(b"PANIC", b"rust panic");
    console::str(b"STAGE", unsafe { STAGE });
    arch::reset(40)
}
