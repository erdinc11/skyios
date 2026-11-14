//! Rust code linked into the SWEET XNU kernel (libsweet_kernel.a).
//!
//! Plain arm64, no NEON/FP (the kernel saves no FP state for its own
//! threads), static relocations, panic = abort. Kernel services come in
//! through the small C ABI in `xnu`.
#![no_std]

mod xnu;

/// Called once from sweet_ufs_rootdev() after the root device is known.
#[unsafe(no_mangle)]
pub extern "C" fn sweet_rs_init() {
    xnu::log(c"SWEET RS: kernel Rust online\n");
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    xnu::panic(c"SWEET RS: Rust panic")
}
