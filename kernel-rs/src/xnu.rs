//! The few XNU services the Rust side calls (osfmk/arm64/sweet_rs.c).
//! Through C hooks, not printf directly: LLVM rewrites printf("%s") into
//! puts, which the kernel does not have.
use core::ffi::{CStr, c_char};

mod sys {
    use core::ffi::c_char;
    unsafe extern "C" {
        pub fn sweet_rs_log(msg: *const c_char);
        pub fn sweet_rs_panic(msg: *const c_char) -> !;
    }
}

pub fn log(msg: &CStr) {
    unsafe { sys::sweet_rs_log(msg.as_ptr()) };
}

pub fn panic(msg: &CStr) -> ! {
    unsafe { sys::sweet_rs_panic(msg.as_ptr() as *const c_char) }
}
