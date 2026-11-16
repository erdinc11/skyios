use crate::xnu::{Dma, Mmio};
use core::cmp::min;

const CE_COUNT: usize = 12;
const CE_PA: u64 = 0x0C80_0000 + 0x24_0000;
const SHADOW_PA: u64 = 0x0C80_0000 + 0x3_2000;

const SR_BASE_LO: usize = 0x00;
const SR_BASE_HI: usize = 0x04;
const SR_SIZE: usize = 0x08;
const DR_BASE_LO: usize = 0x0C;
const DR_BASE_HI: usize = 0x10;
const DR_SIZE: usize = 0x14;
const CTRL1: usize = 0x18;
const SR_WR_IDX: usize = 0x3C;
const DR_WR_IDX: usize = 0x40;
const CUR_DRRI: usize = 0x48;
const SRC_WM: usize = 0x4C;
const DST_WM: usize = 0x50;
const WRAP_RRI_LO: usize = 0xC004;
const WRAP_RRI_HI: usize = 0xC008;

const CTRL1_DMAX: u32 = 0x0000_FFFF;
const CTRL1_SRC_SWAP: u32 = 1 << 17;
const CTRL1_DST_SWAP: u32 = 1 << 18;
const CTRL1_RRI_UPD: u32 = 1 << 19;
const ADDR_HI_MASK: u32 = 0x1F;

pub const RX_BUF_SIZE: usize = 2048;
const RX_POST: usize = 32;
const TX_SLOTS_MAX: usize = 64;
const ATTR_DIS_INTR: u16 = 0x08;
const DESC_SIZE: usize = 16;

struct CeAttr {
    src_n: u16,
    src_max: u16,
    dst_n: u16,
    flags: u16,
}

static CE_ATTR: [CeAttr; CE_COUNT] = [
    CeAttr { src_n: 16, src_max: 2048, dst_n: 0, flags: 0 },
    CeAttr { src_n: 0, src_max: 2048, dst_n: 512, flags: 0 },
    CeAttr { src_n: 0, src_max: 2048, dst_n: 64, flags: 0 },
    CeAttr { src_n: 32, src_max: 2048, dst_n: 0, flags: 0 },
    CeAttr { src_n: 2048, src_max: 256, dst_n: 0, flags: ATTR_DIS_INTR },
    CeAttr { src_n: 0, src_max: 512, dst_n: 512, flags: 0 },
    CeAttr { src_n: 0, src_max: 0, dst_n: 0, flags: 0 },
    CeAttr { src_n: 2, src_max: 2048, dst_n: 2, flags: 0 },
    CeAttr { src_n: 0, src_max: 2048, dst_n: 128, flags: 0 },
    CeAttr { src_n: 0, src_max: 2048, dst_n: 512, flags: 0 },
    CeAttr { src_n: 0, src_max: 2048, dst_n: 512, flags: 0 },
    CeAttr { src_n: 0, src_max: 2048, dst_n: 512, flags: 0 },
];

fn pow2(n: u16) -> u32 {
    let mut p = 1u32;
    while p < n as u32 {
        p <<= 1;
    }
    p
}

fn host_rx(c: usize) -> bool {
    CE_ATTR[c].dst_n != 0 && c != 7
}

fn host_tx(c: usize) -> bool {
    c == 0 || c == 3 || c == 4
}

struct CeRing {
    src_n: u32,
    dst_n: u32,
    src_wr: u32,
    dst_wr: u32,
    dst_sw: u32,
    src_off: usize,
    dst_off: usize,
    rx_off: usize,
    tx_off: usize,
}

impl CeRing {
    fn new() -> Self {
        Self {
            src_n: 0, dst_n: 0, src_wr: 0, dst_wr: 0, dst_sw: 0,
            src_off: 0, dst_off: 0, rx_off: 0, tx_off: 0,
        }
    }

    fn tx_slots(&self) -> u32 {
        min(self.src_n, TX_SLOTS_MAX as u32)
    }
}

pub struct CeEngine {
    regs: Mmio,
    shadow: Mmio,
    mem: Dma,
    rings: [CeRing; CE_COUNT],
    pub tx_count: [u32; CE_COUNT],
    pub rx_count: [u32; CE_COUNT],
}

fn dsb() {
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
}

impl CeEngine {
    pub fn start() -> Option<Self> {
        let regs = Mmio::map(CE_PA, (CE_COUNT + 1) * 0x1000)?;
        let shadow = Mmio::map(SHADOW_PA, 0x1000)?;

        let mut rings: [CeRing; CE_COUNT] = core::array::from_fn(|_| CeRing::new());

        let mut total = 0x1000usize;
        for c in 0..CE_COUNT {
            rings[c].src_n = if CE_ATTR[c].src_n != 0 { pow2(CE_ATTR[c].src_n) } else { 0 };
            rings[c].dst_n = if CE_ATTR[c].dst_n != 0 { pow2(CE_ATTR[c].dst_n) } else { 0 };
            total += ((rings[c].src_n as usize * DESC_SIZE) + 0xfff) & !0xfff;
            total += ((rings[c].dst_n as usize * DESC_SIZE) + 0xfff) & !0xfff;
            if host_rx(c) {
                total += RX_POST * RX_BUF_SIZE;
            }
            if host_tx(c) {
                total += min(rings[c].src_n, TX_SLOTS_MAX as u32) as usize * CE_ATTR[c].src_max as usize;
            }
        }

        let mem = Dma::alloc(total)?;
        mem.zero(0, total);
        crate::log!("SWEET WLAN: ce host memory {:#x} + {:#x}", mem.pa(), total);

        let mut off = 0x1000usize;
        for c in 0..CE_COUNT {
            let r = &mut rings[c];
            if r.src_n != 0 {
                r.src_off = off;
                off += ((r.src_n as usize * DESC_SIZE) + 0xfff) & !0xfff;
            }
            if r.dst_n != 0 {
                r.dst_off = off;
                off += ((r.dst_n as usize * DESC_SIZE) + 0xfff) & !0xfff;
            }
            if host_rx(c) {
                r.rx_off = off;
                off += RX_POST * RX_BUF_SIZE;
            }
            if host_tx(c) {
                let slots = r.tx_slots() as usize;
                r.tx_off = off;
                off += slots * CE_ATTR[c].src_max as usize;
            }
        }

        regs.write(WRAP_RRI_LO, mem.pa() as u32);
        regs.write(WRAP_RRI_HI, (mem.pa() >> 32) as u32 & ADDR_HI_MASK);

        for c in 0..CE_COUNT {
            let v = regs.read(c * 0x1000 + CTRL1);
            regs.write(c * 0x1000 + CTRL1, v | CTRL1_RRI_UPD);
        }

        for c in 0..CE_COUNT {
            let r = &mut rings[c];
            if r.src_n != 0 {
                let pa = mem.pa() + r.src_off as u64;
                r.src_wr = regs.read(c * 0x1000 + SR_WR_IDX) & (r.src_n - 1);
                regs.write(c * 0x1000 + SR_BASE_LO, pa as u32);
                regs.write(c * 0x1000 + SR_BASE_HI, (pa >> 32) as u32 & ADDR_HI_MASK);
                regs.write(c * 0x1000 + SR_SIZE, r.src_n);
                let ctrl = regs.read(c * 0x1000 + CTRL1);
                regs.write(c * 0x1000 + CTRL1,
                    (ctrl & !(CTRL1_DMAX | CTRL1_SRC_SWAP)) | CE_ATTR[c].src_max as u32);
                regs.write(c * 0x1000 + SRC_WM, r.src_n);
            }
            if r.dst_n != 0 {
                let pa = mem.pa() + r.dst_off as u64;
                r.dst_sw = regs.read(c * 0x1000 + CUR_DRRI) & (r.dst_n - 1);
                r.dst_wr = regs.read(c * 0x1000 + DR_WR_IDX) & (r.dst_n - 1);
                regs.write(c * 0x1000 + DR_BASE_LO, pa as u32);
                let hi = regs.read(c * 0x1000 + DR_BASE_HI);
                regs.write(c * 0x1000 + DR_BASE_HI,
                    (hi & !ADDR_HI_MASK) | ((pa >> 32) as u32 & ADDR_HI_MASK));
                regs.write(c * 0x1000 + DR_SIZE, r.dst_n);
                let ctrl = regs.read(c * 0x1000 + CTRL1);
                regs.write(c * 0x1000 + CTRL1, ctrl & !CTRL1_DST_SWAP);
                regs.write(c * 0x1000 + DST_WM, r.dst_n);
            }
        }

        for c in 0..CE_COUNT {
            if !host_rx(c) {
                continue;
            }
            let r = &mut rings[c];
            for _ in 0..RX_POST as u32 {
                let slot = r.dst_wr;
                let desc = r.dst_off + slot as usize * DESC_SIZE;
                let buf_pa = mem.pa() + r.rx_off as u64
                    + (slot % RX_POST as u32) as u64 * RX_BUF_SIZE as u64;
                mem.w64(desc, buf_pa);
                mem.w32(desc + 8, 0);
                r.dst_wr = (slot + 1) & (r.dst_n - 1);
            }
            dsb();
            regs.write(c * 0x1000 + DR_WR_IDX, r.dst_wr);
        }

        crate::log!("SWEET WLAN: ce: {} rings programmed, rx posted", CE_COUNT);
        Some(CeEngine {
            regs, shadow, mem, rings,
            tx_count: [0; CE_COUNT],
            rx_count: [0; CE_COUNT],
        })
    }

    fn post_rx(&mut self, c: usize) {
        let r = &mut self.rings[c];
        let slot = r.dst_wr;
        let desc = r.dst_off + slot as usize * DESC_SIZE;
        let buf_pa = self.mem.pa() + r.rx_off as u64
            + (slot % RX_POST as u32) as u64 * RX_BUF_SIZE as u64;
        self.mem.w64(desc, buf_pa);
        self.mem.w32(desc + 8, 0);
        r.dst_wr = (slot + 1) & (r.dst_n - 1);
    }

    pub fn poll(&mut self, mut callback: impl FnMut(usize, &[u8])) -> bool {
        let mut any = false;
        for c in 0..CE_COUNT {
            if !host_rx(c) {
                continue;
            }
            for _ in 0..16 {
                let r = &self.rings[c];
                let slot = r.dst_sw;
                let desc = r.dst_off + slot as usize * DESC_SIZE;
                let nbytes = self.mem.r32(desc + 8) & 0xFFFF;
                if nbytes == 0 {
                    break;
                }
                dsb();
                self.mem.w32(desc + 8, 0);
                let r = &mut self.rings[c];
                r.dst_sw = (slot + 1) & (r.dst_n - 1);
                self.rx_count[c] += 1;

                let buf_off = r.rx_off + (slot as usize % RX_POST) * RX_BUF_SIZE;
                let data = self.mem.copy_out(buf_off, nbytes as usize);
                callback(c, data);

                self.post_rx(c);
                dsb();
                self.regs.write(c * 0x1000 + DR_WR_IDX, self.rings[c].dst_wr);
                any = true;
            }
        }
        any
    }

    pub fn send(&mut self, c: usize, data: &[u8], transfer_id: u32) -> bool {
        if c >= CE_COUNT || !host_tx(c) || data.len() > CE_ATTR[c].src_max as usize {
            return false;
        }
        let r = &mut self.rings[c];
        if r.src_n == 0 {
            return false;
        }
        let mask = r.src_n - 1;
        let sw = self.mem.r32(c * 4) & 0xFFFF;
        if ((r.src_wr.wrapping_sub(sw)) & mask) >= r.tx_slots() - 1 {
            return false;
        }
        let slot = r.src_wr;
        let tx_sz = CE_ATTR[c].src_max as usize;
        let slot_idx = slot as usize % r.tx_slots() as usize;
        let buf_off = r.tx_off + slot_idx * tx_sz;
        self.mem.copy_in(buf_off, data);
        let desc = r.src_off + slot as usize * DESC_SIZE;
        let buf_pa = self.mem.pa() + buf_off as u64;
        self.mem.w64(desc, buf_pa);
        let flags = ((transfer_id << 4) & 0xFFF0) as u16;
        self.mem.w32(desc + 8, data.len() as u32 | ((flags as u32) << 16));
        self.mem.w32(desc + 12, 0);
        r.src_wr = (slot + 1) & mask;
        self.tx_count[c] += 1;
        dsb();
        self.shadow.write(4 * c, r.src_wr);
        dsb();
        true
    }
}
