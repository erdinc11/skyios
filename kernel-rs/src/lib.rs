#![no_std]

#[macro_use]
mod xnu;
mod gpu;

extern "C" fn gpu_thread(_arg: usize) {
    let secs = xnu::boot_arg_u32(c"sweetgpu_delay").unwrap_or(60);
    xnu::sleep_ms(secs * 1000);
    gpu::bringup();
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_rs_init() {
    xnu::log(c"SWEET RS: kernel Rust online\n");
    if xnu::boot_arg_u32(c"sweetgpu").unwrap_or(0) != 0 {
        xnu::spawn(gpu_thread, 0);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    xnu::panic(c"SWEET RS: Rust panic")
}
