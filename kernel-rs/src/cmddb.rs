use crate::xnu::Mmio;

const LOCATOR: u64 = 0x0c3f_000c;
const MAGIC: u32 = 0x0c03_30db;
const HEADER: usize = 144;
const ENTRY: usize = 24;

pub struct CmdDb {
    db: Mmio,
    size: usize,
}

impl CmdDb {
    pub fn open() -> Option<CmdDb> {
        let page = LOCATOR & !0xfff;
        let off = (LOCATOR - page) as usize;
        let loc = Mmio::map(page, 0x1000)?;
        let pa = loc.read(off) as u64;
        let size = loc.read(off + 4) as usize;
        if pa < 0x8000_0000 || pa & 0xfff != 0 || size < HEADER || size > 0x10_0000 {
            crate::log!("SWEET CMDDB: locator pa {pa:#x} size {size:#x} invalid");
            return None;
        }
        let db = Mmio::map(pa, (size + 0xfff) & !0xfff)?;
        let magic = db.read(4);
        if magic != MAGIC {
            crate::log!("SWEET CMDDB: bad magic {magic:#x}");
            return None;
        }
        crate::log!("SWEET CMDDB: at {pa:#x}, {size:#x} bytes");
        Some(CmdDb { db, size })
    }

    fn r16(&self, off: usize) -> u16 {
        let w = self.db.read(off & !3);
        (if off & 2 != 0 { w >> 16 } else { w }) as u16
    }

    pub fn addr(&self, name: &str) -> Option<u32> {
        let e = self.find(name)?;
        Some(self.db.read(e + 16))
    }

    fn find(&self, name: &str) -> Option<usize> {
        let mut key = [0u8; 8];
        for (i, b) in name.bytes().take(8).enumerate() {
            key[i] = b;
        }
        let k0 = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        let k1 = u32::from_le_bytes([key[4], key[5], key[6], key[7]]);
        for i in 0..8 {
            let h = 8 + 16 * i;
            if self.r16(h) == 0 {
                break;
            }
            let entries = HEADER + self.r16(h + 2) as usize;
            for j in 0..self.r16(h + 6) as usize {
                let e = entries + ENTRY * j;
                if e + ENTRY <= self.size && self.db.read(e) == k0 && self.db.read(e + 4) == k1 {
                    return Some(e);
                }
            }
        }
        None
    }

    pub fn aux(&self, name: &str, out: &mut [u16]) -> usize {
        let mut key = [0u8; 8];
        for (i, b) in name.bytes().take(8).enumerate() {
            key[i] = b;
        }
        let k0 = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        let k1 = u32::from_le_bytes([key[4], key[5], key[6], key[7]]);
        for i in 0..8 {
            let h = 8 + 16 * i;
            if self.r16(h) == 0 {
                break;
            }
            let entries = HEADER + self.r16(h + 2) as usize;
            let data = HEADER + self.r16(h + 4) as usize;
            let count = self.r16(h + 6) as usize;
            for j in 0..count {
                let e = entries + ENTRY * j;
                if e + ENTRY > self.size || self.db.read(e) != k0 || self.db.read(e + 4) != k1 {
                    continue;
                }
                let len = self.r16(e + 20) as usize;
                let start = data + self.r16(e + 22) as usize;
                let mut n = 0;
                while n < out.len() && 2 * n + 2 <= len && start + 2 * n + 2 <= self.size {
                    out[n] = self.r16(start + 2 * n);
                    n += 1;
                }
                while n > 0 && out[n - 1] == 0 {
                    n -= 1;
                }
                return n;
            }
        }
        0
    }
}

pub fn dump(db: &CmdDb, name: &str) {
    let mut lv = [0u16; 16];
    let n = db.aux(name, &mut lv);
    crate::log!("SWEET CMDDB: {name} addr {:#x} {n} levels {:?}", db.addr(name).unwrap_or(0), &lv[..n]);
}
