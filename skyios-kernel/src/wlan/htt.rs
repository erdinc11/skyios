use crate::xnu::Dma;
use super::ce::CeEngine;
use super::htc::HtcState;

const H2T_VERSION_REQ: u8 = 0;
const H2T_RX_RING_CFG: u8 = 2;
const H2T_AGGR_CFG: u8 = 5;
const H2T_FRAG_DESC_BANK_CFG: u8 = 6;
const T2H_VERSION_CONF: u8 = 0x00;
const T2H_RX_IN_ORD_PADDR_IND: u8 = 0x12;
pub const T2H_TX_COMPL_IND: u8 = 0x07;

const HTT_RING_SIZE: usize = 256;
const HTT_BUF_SIZE: usize = 2048;
const HTT_MAX_PENDING_TX: usize = 1056;
const HTT_EXT_DESC_SIZE: usize = 72;

pub const OFF_ATTENTION: u16 = 1;
pub const OFF_FRAG_INFO: u16 = 2;
pub const OFF_MPDU_START: u16 = 4;
pub const OFF_MSDU_START: u16 = 7;
pub const OFF_MSDU_END: u16 = 12;
pub const OFF_MPDU_END: u16 = 26;
pub const OFF_PPDU_START: u16 = 27;
pub const OFF_PPDU_END: u16 = 37;
pub const OFF_HDR_STATUS: u16 = 74;
pub const OFF_MSDU_PAYLOAD: u16 = 90;

pub const TX_SLOTS: usize = 64;
pub const TX_SLOT_SIZE: usize = 2048;

pub struct HttState {
    mem: Option<Dma>,
    ring_off: usize,
    idx_off: usize,
    bank_off: usize,
    buf_off: usize,
    tx_off: usize,
    ext_off: usize,
    alloc: u32,
    pub phase: u8,
    pub req_ms: u32,
    pub msgs: u32,
    pub msdus: u32,
    pub tx_next: u32,
    pub tx_pending: u32,
}

impl HttState {
    pub fn new() -> Self {
        Self {
            mem: None,
            ring_off: 0, idx_off: 0, bank_off: 0, buf_off: 0,
            tx_off: 0, ext_off: 0,
            alloc: 0, phase: 0, req_ms: 0, msgs: 0, msdus: 0,
            tx_next: 0, tx_pending: 0,
        }
    }

    pub fn start(&mut self, htc: &mut HtcState, ce: &mut CeEngine, ms: u32) {
        if self.phase != 0 {
            return;
        }
        let ring = (HTT_RING_SIZE * 8 + 64 + 0xfff) & !0xfff;
        let bank = (HTT_MAX_PENDING_TX * HTT_EXT_DESC_SIZE + 0xfff) & !0xfff;
        let bufs = HTT_RING_SIZE * HTT_BUF_SIZE;
        let tx_area = TX_SLOTS * TX_SLOT_SIZE;
        let ext_area = (TX_SLOTS * HTT_EXT_DESC_SIZE + 0xfff) & !0xfff;
        let total = ring + bank + bufs + tx_area + ext_area;
        let d = match Dma::alloc(total) {
            Some(d) => d,
            None => {
                crate::log!("SWEET WLAN: htt: no memory");
                return;
            }
        };
        d.zero(0, total);
        self.ring_off = 0;
        self.idx_off = HTT_RING_SIZE * 8;
        self.bank_off = ring;
        self.buf_off = ring + bank;
        self.tx_off = ring + bank + bufs;
        self.ext_off = ring + bank + bufs + tx_area;
        self.mem = Some(d);
        self.phase = 1;
        self.req_ms = ms;
        let m = [H2T_VERSION_REQ, 0, 0, 0];
        let ok = htc.htt_send(ce, &m);
        crate::log!("SWEET WLAN: htt: VERSION_REQ {}", if ok { "sent" } else { "FAILED" });
    }

    pub fn configure(&mut self, htc: &mut HtcState, ce: &mut CeEngine, why: &str) {
        let d = match self.mem {
            Some(ref d) => d,
            None => return,
        };
        self.phase = 2;

        let bank_pa = d.pa() + self.bank_off as u64;
        let mut m = [0u8; 64];
        m[0] = H2T_FRAG_DESC_BANK_CFG;
        m[2] = 1;
        m[3] = HTT_EXT_DESC_SIZE as u8;
        put64(&mut m, 4, bank_pa);
        put16(&mut m, 4 + 4 * 8, 0);
        put16(&mut m, 4 + 4 * 8 + 2, (HTT_MAX_PENDING_TX - 1) as u16);
        m[4 + 4 * 8 + 4 + 3 * 4 + 4 + 2 + 2] = 1;
        let a = htc.htt_send(ce, &m);

        let buf_pa = d.pa() + self.buf_off as u64;
        for i in 0..(HTT_RING_SIZE - 1) as u32 {
            let pa = buf_pa + i as u64 * HTT_BUF_SIZE as u64;
            d.w64(self.ring_off + self.alloc as usize * 8, pa);
            self.alloc = (self.alloc + 1) & (HTT_RING_SIZE as u32 - 1);
        }

        let ring_pa = d.pa() + self.ring_off as u64;
        let idx_pa = d.pa() + self.idx_off as u64;
        let mut r = [0u8; 48];
        r[0] = H2T_RX_RING_CFG;
        r[1] = 1;
        put64(&mut r, 4, idx_pa);
        put64(&mut r, 12, ring_pa);
        put16(&mut r, 20, HTT_RING_SIZE as u16);
        put16(&mut r, 22, HTT_BUF_SIZE as u16);
        put16(&mut r, 24, 0xFFFF);
        put16(&mut r, 26, 0);
        put16(&mut r, 28, OFF_HDR_STATUS);
        put16(&mut r, 30, OFF_MSDU_PAYLOAD);
        put16(&mut r, 32, OFF_PPDU_START);
        put16(&mut r, 34, OFF_PPDU_END);
        put16(&mut r, 36, OFF_MPDU_START);
        put16(&mut r, 38, OFF_MPDU_END);
        put16(&mut r, 40, OFF_MSDU_START);
        put16(&mut r, 42, OFF_MSDU_END);
        put16(&mut r, 44, OFF_ATTENTION);
        put16(&mut r, 46, OFF_FRAG_INFO);

        self.publish();
        let b = htc.htt_send(ce, &r);

        let g = [H2T_AGGR_CFG, 64, 3, 0];
        let c = htc.htt_send(ce, &g);
        crate::log!(
            "SWEET WLAN: htt ({}): frag bank {}, rx ring {}, aggr {}",
            why,
            if a { "ok" } else { "FAILED" },
            if b { "ok" } else { "FAILED" },
            if c { "ok" } else { "FAILED" }
        );
    }

    fn post(&mut self, pa: u64) {
        let d = match self.mem {
            Some(ref d) => d,
            None => return,
        };
        let buf_pa = d.pa() + self.buf_off as u64;
        let buf_end = buf_pa + (HTT_RING_SIZE * HTT_BUF_SIZE) as u64;
        if pa >= buf_pa && pa < buf_end {
            let off = (pa - buf_pa) as usize;
            d.w32(self.buf_off + off + 4 * OFF_ATTENTION as usize, 0);
        }
        d.w64(self.ring_off + self.alloc as usize * 8, pa);
        self.alloc = (self.alloc + 1) & (HTT_RING_SIZE as u32 - 1);
    }

    fn publish(&self) {
        let d = match self.mem {
            Some(ref d) => d,
            None => return,
        };
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
        d.w32(self.idx_off, self.alloc);
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    }

    pub fn rx(&mut self, htc: &mut HtcState, ce: &mut CeEngine, p: &[u8], _ms: u32) {
        if p.is_empty() {
            return;
        }
        let t = p[0];
        self.msgs += 1;
        match t {
            T2H_VERSION_CONF => {
                if p.len() >= 3 {
                    crate::log!("SWEET WLAN: htt: VERSION_CONF {}.{}", p[2], p[1]);
                }
                if self.phase == 1 {
                    self.configure(htc, ce, "version conf");
                }
            }
            T2H_RX_IN_ORD_PADDR_IND => {
                self.rx_in_ord(p);
            }
            T2H_TX_COMPL_IND => {
                self.tx_compl(p);
            }
            _ => {}
        }
    }

    fn rx_in_ord(&mut self, p: &[u8]) {
        if p.len() < 8 {
            return;
        }
        let n = u16::from_le_bytes([p[6], p[7]]) as u32;
        let mut off = 8usize;
        for _ in 0..n {
            if off + 12 > p.len() {
                break;
            }
            let pa = u64::from_le_bytes([
                p[off], p[off + 1], p[off + 2], p[off + 3],
                p[off + 4], p[off + 5], p[off + 6], p[off + 7],
            ]);
            off += 12;
            self.msdus += 1;
            self.post(pa);
        }
        self.publish();
    }

    fn tx_compl(&mut self, p: &[u8]) {
        if p.len() < 4 {
            return;
        }
        let n = (p[1] as u32) | ((p[2] as u32) << 8);
        let count = core::cmp::min(n, (p.len() as u32 - 4) / 2);
        if self.tx_pending >= count {
            self.tx_pending -= count;
        }
    }

    pub fn poll(&mut self, htc: &mut HtcState, ce: &mut CeEngine, ms: u32) {
        if self.phase == 1 && ms.wrapping_sub(self.req_ms) > 2000 {
            crate::log!("SWEET WLAN: htt: no VERSION_CONF in 2 s, configuring anyway");
            self.configure(htc, ce, "timeout");
        }
    }

    pub fn tx_frame(&mut self, htc: &mut HtcState, ce: &mut CeEngine, frame: &[u8]) -> bool {
        let d = match self.mem {
            Some(ref d) => d,
            None => return false,
        };
        if frame.len() > TX_SLOT_SIZE - 32 || self.tx_pending >= TX_SLOTS as u32 {
            return false;
        }
        let slot = self.tx_next as usize % TX_SLOTS;
        let ext_pa = d.pa() + self.ext_off as u64 + (slot * HTT_EXT_DESC_SIZE) as u64;

        let hdr_len = 32;
        let mut hdr = [0u8; 32];
        hdr[0] = 0x01;
        put16(&mut hdr, 2, slot as u16);
        put16(&mut hdr, 4, frame.len() as u16);
        put32(&mut hdr, 8, ext_pa as u32);
        put32(&mut hdr, 12, (ext_pa >> 32) as u32);
        hdr[16] = 0x00;

        let buf_off = self.tx_off + slot * TX_SLOT_SIZE;
        d.copy_in(buf_off, &hdr);
        d.copy_in(buf_off + hdr_len, frame);

        let ext_off = self.ext_off + slot * HTT_EXT_DESC_SIZE;
        let frag_pa = d.pa() + (buf_off + hdr_len) as u64;
        d.w32(ext_off, frag_pa as u32);
        d.w32(ext_off + 4, (frag_pa >> 32) as u32);
        d.w32(ext_off + 8, frame.len() as u32);

        let total = hdr_len + frame.len();
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
        let ok = htc.htt_send_buf(ce, d.copy_out(buf_off, total));
        if ok {
            self.tx_next += 1;
            self.tx_pending += 1;
        }
        ok
    }
}

fn put16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}
