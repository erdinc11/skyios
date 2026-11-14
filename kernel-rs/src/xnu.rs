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
