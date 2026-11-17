#![no_std]

#[macro_use]
mod xnu;
mod cmddb;
mod cp;
mod gmumem;
mod gpu;
mod hfi;
mod scm;
mod smmu;
mod wlan;
mod zap;

extern "C" fn gpu_thread(stage: usize) {
    let secs = xnu::boot_arg_u32(c"sweetgpu_delay").unwrap_or(60);
    xnu::sleep_ms(secs * 1000);
    if stage >= 3 {
        if let Some(db) = cmddb::CmdDb::open() {
            for name in ["gfx.lvl", "mx.lvl", "cx.lvl", "ebi.lvl", "lmx.lvl", "lcx.lvl", "MC0", "SH0"] {
                cmddb::dump(&db, name);
            }
        }
    }
    gpu::bringup(stage as u32);
    if stage >= 3 {
        smmu::dump();
    }
}

// sweetreboot=S: S seconds after boot reset into the bootloader (fastboot),
// as the shim does, so test boots come back without a button press
const IMEM_PAGE: u64 = 0x146a_a000;
const IMEM_RESTART_REASON: usize = 0x65c;
const RESTART_BOOTLOADER: u32 = 0x7766_5500;
const PSHOLD: u64 = 0x0c26_4000;

extern "C" fn reboot_thread(secs: usize) {
    xnu::sleep_ms(secs as u32 * 1000);
    crate::log!("SWEET RS: sweetreboot: {secs} s up, resetting into fastboot");
    // give the console 3 s to reach the host
    xnu::sleep_ms(3000);
    if let Some(imem) = xnu::Mmio::map(IMEM_PAGE, 0x1000) {
        imem.write(IMEM_RESTART_REASON, RESTART_BOOTLOADER);
    }
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    scm::psci_system_reset();
    if let Some(ps) = xnu::Mmio::map(PSHOLD, 0x1000) {
        ps.write(0, 0);
    }
}

// sweetwdog=S: arm the APSS watchdog (qcom,wdt@17c10000 in the sweet DT,
// 32768 Hz, register layout of Linux qcom_wdt "kpss") with an S second bite
// and pet it every second, so a hung or panicked kernel resets the phone
// (observed hardware resets land in recovery) instead of waiting for a hand
const APSS_WDT: u64 = 0x17c1_0000;
const WDT_RST: usize = 0x04;
const WDT_EN: usize = 0x08;
const WDT_BARK_TIME: usize = 0x10;
const WDT_BITE_TIME: usize = 0x14;

extern "C" fn wdog_thread(secs: usize) {
    let Some(wdt) = xnu::Mmio::map(APSS_WDT, 0x1000) else {
        return;
    };
    let ticks = secs as u32 * 32768;
    wdt.write(WDT_EN, 0);
    wdt.write(WDT_RST, 1);
    wdt.write(WDT_BARK_TIME, ticks);
    wdt.write(WDT_BITE_TIME, ticks);
    wdt.write(WDT_EN, 1);
    crate::log!("SWEET RS: watchdog armed, bite after {secs} s without a pet (en {:#x})", wdt.read(WDT_EN));
    loop {
        wdt.write(WDT_RST, 1);
        xnu::sleep_ms(1000);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_rs_init() {
    if let Some(secs) = xnu::boot_arg_u32(c"sweetwdog").filter(|&s| s >= 5) {
        xnu::spawn(wdog_thread, secs as usize);
    }
    if let Some(secs) = xnu::boot_arg_u32(c"sweetreboot").filter(|&s| s != 0) {
        xnu::spawn(reboot_thread, secs as usize);
    }
    xnu::log(c"SWEET RS: kernel Rust online\n");
    let stage = xnu::boot_arg_u32(c"sweetgpu").unwrap_or(0);
    if stage != 0 {
        xnu::spawn(gpu_thread, stage as usize);
    }
    let wifi_arg = xnu::boot_arg_u32(c"sweetwifi").unwrap_or(0);
    xnu::logf(format_args!("SWEET RS: sweetwifi={}\n", wifi_arg));
    if wifi_arg != 0 {
        unsafe extern "C" { fn sweet_sta_init(); }
        unsafe { sweet_sta_init() };
        xnu::spawn(wlan::wlan_thread, 0);
        xnu::log(c"SWEET RS: wlan thread spawned\n");
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    xnu::panic(c"SWEET RS: Rust panic")
}
