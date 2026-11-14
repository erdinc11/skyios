use core::arch::asm;

const IMEM_RESTART_REASON: usize = 0x146a_a65c;
const RESTART_BOOTLOADER: u32 = 0x7766_5500;
const PSHOLD: usize = 0x0c26_4000;
const PSCI_SYSTEM_RESET: u64 = 0x8400_0009;

macro_rules! sysreg {
    ($name:literal) => {{
        let v: u64;
        unsafe { core::arch::asm!(concat!("mrs {}, ", $name), out(reg) v, options(nomem, nostack)) };
        v
    }};
}
pub(crate) use sysreg;

pub fn rd32(a: usize) -> u32 {
    unsafe { core::ptr::read_volatile(a as *const u32) }
}

pub fn wr32(a: usize, v: u32) {
    unsafe { core::ptr::write_volatile(a as *mut u32, v) }
}

pub fn dsb() {
    unsafe { asm!("dsb sy", options(nostack)) };
}

pub fn smc(f: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let mut x0 = f;
    unsafe {
        asm!(
            "smc #0",
            inout("x0") x0,
            inout("x1") a1 => _,
            inout("x2") a2 => _,
            inout("x3") a3 => _,
            out("x4") _, out("x5") _, out("x6") _, out("x7") _,
            out("x8") _, out("x9") _, out("x10") _, out("x11") _,
            out("x12") _, out("x13") _, out("x14") _, out("x15") _,
            out("x16") _, out("x17") _,
            options(nostack)
        )
    };
    x0
}

pub fn now() -> u64 {
    unsafe { asm!("isb", options(nostack)) };
    sysreg!("cntpct_el0")
}

pub fn ticks_virtual() -> u64 {
    unsafe { asm!("isb", options(nostack)) };
    sysreg!("cntvct_el0")
}

pub fn delay_ms(ms: u64) {
    let end = now() + sysreg!("cntfrq_el0") / 1000 * ms;
    while now() < end {}
}

pub fn reset(after_s: u64) -> ! {
    crate::console::skip();
    crate::console::str(b"RESET", if after_s > 20 { b"in 40s" } else { b"in 20s" });
    delay_ms(after_s * 1000);
    wr32(IMEM_RESTART_REASON, RESTART_BOOTLOADER);
    dsb();
    smc(PSCI_SYSTEM_RESET, 0, 0, 0);
    wr32(PSHOLD, 0);
    loop {
        unsafe { asm!("wfe", options(nomem, nostack)) };
    }
}
