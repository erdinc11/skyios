use core::ffi::{CStr, c_char};
use core::fmt::{self, Write};

mod sys {
    use core::ffi::c_char;
    unsafe extern "C" {
        pub fn sweet_rs_log(msg: *const c_char);
        pub fn sweet_rs_panic(msg: *const c_char) -> !;
        pub fn sweet_rs_io_map(pa: u64, size: u64) -> usize;
        pub fn sweet_rs_boot_arg_u32(name: *const c_char, out: *mut u32) -> bool;
        pub fn sweet_rs_delay_us(us: u32);
        pub fn sweet_rs_sleep_ms(ms: u32);
        pub fn sweet_rs_spawn(f: extern "C" fn(usize), arg: usize) -> bool;
        pub fn sweet_rs_alloc_contig(size: u64, pa: *mut u64) -> usize;
        pub fn sweet_rs_cache_clean(va: usize, size: u64);
        pub fn sweet_rs_cache_flush(va: usize, size: u64);
    }
}

pub fn log(msg: &CStr) {
    unsafe { sys::sweet_rs_log(msg.as_ptr()) };
}

pub fn panic(msg: &CStr) -> ! {
    unsafe { sys::sweet_rs_panic(msg.as_ptr() as *const c_char) }
}

struct Line {
    buf: [u8; 256],
    len: usize,
}

impl Write for Line {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() - 2 && b != 0 {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        Ok(())
    }
}

pub fn logf(args: fmt::Arguments) {
    let mut l = Line { buf: [0; 256], len: 0 };
    let _ = l.write_fmt(args);
    l.buf[l.len] = b'\n';
    l.buf[l.len + 1] = 0;
    unsafe { sys::sweet_rs_log(l.buf.as_ptr() as *const c_char) };
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::xnu::logf(format_args!($($t)*)) };
}

pub fn boot_arg_u32(name: &CStr) -> Option<u32> {
    let mut v = 0u32;
    unsafe { sys::sweet_rs_boot_arg_u32(name.as_ptr(), &mut v) }.then_some(v)
}

pub fn delay_us(us: u32) {
    unsafe { sys::sweet_rs_delay_us(us) }
}

pub fn sleep_ms(ms: u32) {
    unsafe { sys::sweet_rs_sleep_ms(ms) }
}

pub fn spawn(f: extern "C" fn(usize), arg: usize) -> bool {
    unsafe { sys::sweet_rs_spawn(f, arg) }
}

pub struct Mmio {
    base: usize,
    size: usize,
}

impl Mmio {
    pub fn map(pa: u64, size: usize) -> Option<Mmio> {
        let base = unsafe { sys::sweet_rs_io_map(pa, size as u64) };
        (base != 0).then_some(Mmio { base, size })
    }

    pub fn read(&self, off: usize) -> u32 {
        assert!(off + 4 <= self.size && off % 4 == 0);
        let v = unsafe { core::ptr::read_volatile((self.base + off) as *const u32) };
        unsafe { core::arch::asm!("dmb oshld", options(nostack, preserves_flags)) };
        v
    }

    pub fn write(&self, off: usize, v: u32) {
        assert!(off + 4 <= self.size && off % 4 == 0);
        unsafe { core::arch::asm!("dmb oshst", options(nostack, preserves_flags)) };
        unsafe { core::ptr::write_volatile((self.base + off) as *mut u32, v) };
    }
}

pub struct Dma {
    pub va: usize,
    pub pa: u64,
    pub size: usize,
}

impl Dma {
    pub fn alloc(size: usize) -> Option<Dma> {
        let mut pa = 0u64;
        let va = unsafe { sys::sweet_rs_alloc_contig(size as u64, &mut pa) };
        (va != 0).then_some(Dma { va, pa, size })
    }

    pub fn w32(&self, off: usize, v: u32) {
        assert!(off + 4 <= self.size && off % 4 == 0);
        unsafe { core::ptr::write_volatile((self.va + off) as *mut u32, v) };
    }

    pub fn r32(&self, off: usize) -> u32 {
        assert!(off + 4 <= self.size && off % 4 == 0);
        unsafe { core::ptr::read_volatile((self.va + off) as *const u32) }
    }

    pub fn w64(&self, off: usize, v: u64) {
        assert!(off + 8 <= self.size && off % 8 == 0);
        unsafe { core::ptr::write_volatile((self.va + off) as *mut u64, v) };
    }

    pub fn clean(&self) {
        unsafe { sys::sweet_rs_cache_clean(self.va, self.size as u64) };
    }

    pub fn flush(&self, off: usize, len: usize) {
        unsafe { sys::sweet_rs_cache_flush(self.va + off, len as u64) };
    }
}
