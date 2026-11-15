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

#[unsafe(no_mangle)]
pub extern "C" fn sweet_rs_init() {
    xnu::log(c"SWEET RS: kernel Rust online\n");
    let stage = xnu::boot_arg_u32(c"sweetgpu").unwrap_or(0);
    if stage != 0 {
        xnu::spawn(gpu_thread, stage as usize);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    xnu::panic(c"SWEET RS: Rust panic")
}
