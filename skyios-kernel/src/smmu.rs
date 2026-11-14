use crate::xnu::Mmio;

const SMMU: u64 = 0x0504_0000;
const SMMU_SIZE: usize = 0x10000;

pub fn dump() {
    let Some(s) = Mmio::map(SMMU, SMMU_SIZE) else {
        crate::log!("SWEET SMMU: map failed");
        return;
    };
    let cr0 = s.read(0x0);
    let id0 = s.read(0x20);
    let id1 = s.read(0x24);
    let id2 = s.read(0x28);
    crate::log!("SWEET SMMU: sCR0 {cr0:#010x} IDR0 {id0:#010x} IDR1 {id1:#010x} IDR2 {id2:#010x}");
    let page = if id1 & (1 << 31) != 0 { 0x10000 } else { 0x1000 };
    let npage = 1usize << (((id1 >> 28) & 7) + 1);
    let ncb = (id1 & 0xff) as usize;
    let nsmr = (id0 & 0xff) as usize;
    crate::log!("SWEET SMMU: page {page:#x} global pages {npage} context banks {ncb} stream match groups {nsmr}");
    for i in 0..nsmr.min(16) {
        let smr = s.read(0x800 + 4 * i);
        let s2cr = s.read(0xc00 + 4 * i);
        if smr & (1 << 31) != 0 || s2cr != 0 {
            crate::log!("SWEET SMMU: smr[{i}] {smr:#010x} s2cr {s2cr:#010x}");
        }
    }
    let cb_base = npage * page;
    if cb_base + ncb * page > SMMU_SIZE {
        crate::log!("SWEET SMMU: context banks at {cb_base:#x} lie outside the mapping");
        return;
    }
    for i in 0..ncb.min(8) {
        let cb = cb_base + i * page;
        let cbar = s.read(0x1000 + 4 * i);
        let sctlr = s.read(cb);
        let ttbr0 = (s.read(cb + 0x24) as u64) << 32 | s.read(cb + 0x20) as u64;
        let fsr = s.read(cb + 0x58);
        crate::log!("SWEET SMMU: cb{i} cbar {cbar:#010x} sctlr {sctlr:#010x} ttbr0 {ttbr0:#018x} fsr {fsr:#x}");
    }
}
