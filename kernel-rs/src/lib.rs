#![no_std]

#[macro_use]
mod xnu;
mod gpu;

#[unsafe(no_mangle)]
pub extern "C" fn sweet_rs_init() {
    xnu::log(c"SWEET RS: kernel Rust online\n");
    if xnu::boot_arg_u32(c"sweetgpu").unwrap_or(0) != 0 {
        gpu::probe();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    xnu::panic(c"SWEET RS: Rust panic")
}
