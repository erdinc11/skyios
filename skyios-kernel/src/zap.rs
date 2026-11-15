// Adreno zap shader: load the signed a615_zap image into the GPU carve-out and
// have TZ authenticate it (PAS id 13), so the CP may later leave secure mode
// with CP_SET_SECURE_MODE(0). Follows Linux adreno_zap_shader_load() and
// qcom_mdt_load() (drivers/soc/qcom/mdt_loader.c). The firmware stays out of
// git: SWEET_ZAP_MDT / SWEET_ZAP_B02 point at research/gpu-fw/a615_zap.*.

use crate::scm;
use crate::xnu::{Dma, Mmio};

const PAS_ID: u64 = 13;
// DT /reserved-memory/gpu_region@97715000 (no-map, 8 KiB, low DDR bank that
// XNU does not use)
const ZAP_PA: u64 = 0x9771_5000;
const ZAP_SIZE: usize = 0x2000;

const SVC_INFO: u32 = 0x06;
const INFO_IS_CALL_AVAIL: u32 = 0x01;
const SVC_PIL: u32 = 0x02;
const PAS_INIT_IMAGE: u32 = 0x01;
const PAS_MEM_SETUP: u32 = 0x02;
const PAS_AUTH_AND_RESET: u32 = 0x05;
const PAS_IS_SUPPORTED: u32 = 0x07;

const PT_LOAD: u32 = 1;
const MDT_TYPE_MASK: u32 = 7 << 24;
const MDT_TYPE_HASH: u32 = 2 << 24;
const MDT_RELOCATABLE: u32 = 1 << 27;

#[cfg(feature = "zapfw")]
static MDT: &[u8] = include_bytes!(env!("SWEET_ZAP_MDT"));
#[cfg(feature = "zapfw")]
static B02: &[u8] = include_bytes!(env!("SWEET_ZAP_B02"));
#[cfg(not(feature = "zapfw"))]
static MDT: &[u8] = &[];
#[cfg(not(feature = "zapfw"))]
static B02: &[u8] = &[];

static mut LOADED: bool = false;

fn u16le(b: &[u8], o: usize) -> u32 {
    u16::from_le_bytes([b[o], b[o + 1]]) as u32
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

struct Seg {
    index: usize,
    ptype: u32,
    paddr: u32,
    filesz: u32,
    memsz: u32,
    flags: u32,
}

// ELF32 program headers of the mdt
fn segments(f: &mut [Option<Seg>; 8]) -> Option<(usize, usize)> {
    if MDT.len() < 52 || &MDT[..4] != b"\x7fELF" || MDT[4] != 1 {
        return None;
    }
    let (phoff, phentsize, phnum) = (u32le(MDT, 28) as usize, u16le(MDT, 42) as usize, u16le(MDT, 44) as usize);
    if phentsize != 32 || phnum > f.len() || phoff + phnum * 32 > MDT.len() {
        return None;
    }
    for (i, slot) in f.iter_mut().enumerate().take(phnum) {
        let p = phoff + i * 32;
        *slot = Some(Seg {
            index: i,
            ptype: u32le(MDT, p),
            paddr: u32le(MDT, p + 12),
            filesz: u32le(MDT, p + 16),
            memsz: u32le(MDT, p + 20),
            flags: u32le(MDT, p + 24),
        });
    }
    Some((phoff + phnum * 32, phnum))
}

// mdt_phdr_valid()
fn valid(s: &Seg) -> bool {
    s.ptype == PT_LOAD && s.memsz != 0 && s.flags & MDT_TYPE_MASK != MDT_TYPE_HASH
}

pub fn load() -> bool {
    if unsafe { LOADED } {
        return true;
    }
    if MDT.is_empty() {
        crate::log!("SWEET ZAP: no zap firmware in this kernel");
        return false;
    }
    // Diagnostics only: 0110f got is_supported(13) = 0 from TZ. Linux
    // adreno_zap_shader_load() does not ask, it goes straight to init_image.
    for id in [1u64, 4, PAS_ID] {
        let (st, r) = scm::call(SVC_PIL, PAS_IS_SUPPORTED, scm::args(&[scm::VAL]), &[id]);
        crate::log!("SWEET ZAP: pas {id} is_supported -> status {st} result {}", r[0]);
    }
    let (st, r) = scm::call(
        SVC_INFO,
        INFO_IS_CALL_AVAIL,
        scm::args(&[scm::VAL]),
        &[(2 << 24) | ((SVC_PIL << 8) | PAS_INIT_IMAGE) as u64],
    );
    crate::log!("SWEET ZAP: is_call_avail(pil init_image) -> status {st} result {}", r[0]);
    let mut segs: [Option<Seg>; 8] = Default::default();
    let Some((ehdr_size, _)) = segments(&mut segs) else {
        crate::log!("SWEET ZAP: bad mdt");
        return false;
    };
    let (mut lo, mut hi, mut reloc) = (u32::MAX, 0u32, false);
    for s in segs.iter().flatten().filter(|s| valid(s)) {
        reloc |= s.flags & MDT_RELOCATABLE != 0;
        lo = lo.min(s.paddr);
        hi = hi.max((s.paddr + s.memsz + 0xfff) & !0xfff);
    }
    let span = hi.wrapping_sub(lo) as usize;
    if !reloc || lo > hi || span > ZAP_SIZE {
        crate::log!("SWEET ZAP: unexpected layout lo {lo:#x} hi {hi:#x} reloc {reloc}");
        return false;
    }

    // qcom_mdt_read_metadata(): ELF header + phdrs, then the hash segment;
    // the .mdt holds exactly those two, back to back
    let Some(meta) = Dma::alloc(MDT.len().next_multiple_of(0x1000)) else {
        return false;
    };
    for (i, w) in MDT.chunks(4).enumerate() {
        let mut b = [0u8; 4];
        b[..w.len()].copy_from_slice(w);
        meta.w32(4 * i, u32::from_le_bytes(b));
    }
    meta.clean();
    let (st, r) = scm::call(
        SVC_PIL,
        PAS_INIT_IMAGE,
        scm::args(&[scm::VAL, scm::RW]),
        &[PAS_ID, meta.pa],
    );
    crate::log!(
        "SWEET ZAP: init_image (metadata {} bytes at {:#x}, ehdr {ehdr_size}) -> status {st} result {:#x}",
        MDT.len(),
        meta.pa,
        r[0]
    );
    if st != 0 || r[0] != 0 {
        return false;
    }
    let (st, r) = scm::call(
        SVC_PIL,
        PAS_MEM_SETUP,
        scm::args(&[scm::VAL, scm::VAL, scm::VAL]),
        &[PAS_ID, ZAP_PA, span as u64],
    );
    crate::log!("SWEET ZAP: mem_setup {ZAP_PA:#x}+{span:#x} -> status {st} result {:#x}", r[0]);
    if st != 0 || r[0] != 0 {
        return false;
    }

    let Some(mem) = Mmio::map(ZAP_PA, ZAP_SIZE) else {
        return false;
    };
    for s in segs.iter().flatten().filter(|s| valid(s)) {
        let off = (s.paddr - lo) as usize;
        // only the a615_zap layout is embedded: one load segment, .b02
        if s.index != 2 || s.filesz as usize != B02.len() {
            crate::log!("SWEET ZAP: segment {} ({} bytes) not embedded", s.index, s.filesz);
            return false;
        }
        for i in 0..(s.memsz as usize).div_ceil(4) {
            let mut b = [0u8; 4];
            for (j, x) in b.iter_mut().enumerate() {
                *x = B02.get(4 * i + j).copied().unwrap_or(0);
            }
            mem.write(off + 4 * i, u32::from_le_bytes(b));
        }
    }
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    crate::log!("SWEET ZAP: segment 2 ({} bytes) at {ZAP_PA:#x}, first word {:#010x}", B02.len(), mem.read(0));

    let (st, r) = scm::call(SVC_PIL, PAS_AUTH_AND_RESET, scm::args(&[scm::VAL]), &[PAS_ID]);
    crate::log!("SWEET ZAP: auth_and_reset -> status {st} result {:#x}", r[0]);
    if st != 0 || r[0] != 0 {
        return false;
    }
    unsafe { LOADED = true };
    true
}
