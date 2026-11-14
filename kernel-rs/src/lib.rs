#![no_std]

mod xnu;

#[unsafe(no_mangle)]
pub extern "C" fn sweet_rs_init() {
    xnu::log(c"SWEET RS: kernel Rust online\n");
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    xnu::panic(c"SWEET RS: Rust panic")
}
