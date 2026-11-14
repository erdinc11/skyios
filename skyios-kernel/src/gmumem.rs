use crate::xnu::{Dma, Mmio, delay_us};

const SMMU: u64 = 0x0504_0000;
const SMMU_SIZE: usize = 0x10000;
const PAGE: usize = 0x1000;
const CB_BASE: usize = 0x8000;
const CB: usize = 4;
const SMR: usize = 4;
const GMU_SID: u32 = 5;

pub const IOVA_BASE: u32 = 0x6000_0000;
pub const DEBUG_IOVA: u32 = 0x6000_1000;
pub const LOG_IOVA: u32 = 0x6000_5000;
pub const HFI_IOVA: u32 = 0x6000_9000;
pub const REGION_SIZE: usize = 0x4000;
const WINDOW: usize = 0xd000;
const TABLES: usize = 3 * PAGE;

pub struct GmuMem {
    pub dma: Dma,
}

impl GmuMem {
    pub fn off(&self, iova: u32) -> usize {
        TABLES + (iova - IOVA_BASE) as usize
    }
}

fn pte_page(pa: u64) -> u64 {
    pa | (1 << 10) | (1 << 6) | 0b11
}

fn pte_table(pa: u64) -> u64 {
    pa | 0b11
}

pub fn setup() -> Option<GmuMem> {
    let smmu = Mmio::map(SMMU, SMMU_SIZE)?;
    let Some(dma) = Dma::alloc(TABLES + WINDOW) else {
        crate::log!("SWEET GMUMEM: contiguous alloc failed");
        return None;
    };
    let l1 = dma.pa;
    let l2 = dma.pa + PAGE as u64;
    let l3 = dma.pa + 2 * PAGE as u64;
    let iova = IOVA_BASE as usize;
    dma.w64(((iova >> 30) & 0x1ff) * 8, pte_table(l2));
    dma.w64(PAGE + ((iova >> 21) & 0x1ff) * 8, pte_table(l3));
    for i in 0..WINDOW / PAGE {
        dma.w64(2 * PAGE + (((iova >> 12) & 0x1ff) + i) * 8, pte_page(dma.pa + (TABLES + i * PAGE) as u64));
    }
    dma.clean();
    let cb = CB_BASE + CB * PAGE;
    smmu.write(cb, 0);
    smmu.write(PAGE + 0x800 + 4 * CB, 1);
    smmu.write(PAGE + 4 * CB, (1 << 16) | (0xf << 12) | (3 << 8));
    smmu.write(cb + 0x10, (7 << 15) | 2);
    smmu.write(cb + 0x30, (1 << 23) | 32);
    smmu.write(cb + 0x20, l1 as u32);
    smmu.write(cb + 0x24, (l1 >> 32) as u32);
    smmu.write(cb + 0x38, 0x44);
    smmu.write(cb + 0x3c, 0);
    smmu.write(cb + 0x58, smmu.read(cb + 0x58));
    smmu.write(0x68, 0);
    smmu.write(0x70, 0);
    for _ in 0..1000 {
        if smmu.read(0x74) & 1 == 0 {
            break;
        }
        delay_us(1);
    }
    smmu.write(cb, (1 << 6) | (1 << 5) | (1 << 2) | (1 << 1) | 1);
    smmu.write(0xc00 + 4 * SMR, CB as u32);
    smmu.write(0x800 + 4 * SMR, (1 << 31) | GMU_SID);
    crate::log!(
        "SWEET GMUMEM: {:#x} bytes at pa {:#x}, iova {IOVA_BASE:#x}+{WINDOW:#x} via cb{CB} (sctlr {:#010x} ttbr0 {:#x}), smr[{SMR}] {:#010x} s2cr {:#010x}",
        dma.size,
        dma.pa,
        smmu.read(cb),
        (smmu.read(cb + 0x24) as u64) << 32 | smmu.read(cb + 0x20) as u64,
        smmu.read(0x800 + 4 * SMR),
        smmu.read(0xc00 + 4 * SMR)
    );
    Some(GmuMem { dma })
}

pub fn fault_report() {
    let Some(smmu) = Mmio::map(SMMU, SMMU_SIZE) else {
        return;
    };
    let cb = CB_BASE + CB * PAGE;
    let fsr = smmu.read(cb + 0x58);
    let far = (smmu.read(cb + 0x64) as u64) << 32 | smmu.read(cb + 0x60) as u64;
    crate::log!(
        "SWEET GMUMEM: cb{CB} fsr {fsr:#x} far {far:#x} fsynr0 {:#x}; gfsr {:#x} gfsynr1 {:#x}",
        smmu.read(cb + 0x68),
        smmu.read(0x48),
        smmu.read(0x54)
    );
}

pub fn hfi_init(m: &GmuMem) {
    let d = &m.dma;
    let base = m.off(HFI_IOVA);
    for i in 0..0x150 {
        d.w32(base + 4 * i, 0);
    }
    d.clean();
}

pub fn dump(m: &GmuMem) {
    let d = &m.dma;
    d.flush(TABLES, WINDOW);
    let hfi = m.off(HFI_IOVA);
    for i in 0..3 {
        let h = hfi + 24 + i * 48;
        crate::log!(
            "SWEET GMUMEM: hfi q{i} status {} type {:#x} read {} write {} dropped {}",
            d.r32(h),
            d.r32(h + 8),
            d.r32(h + 40),
            d.r32(h + 44),
            d.r32(h + 20)
        );
    }
    for (name, base) in [("log", m.off(LOG_IOVA)), ("dbgq", hfi + 0x3000), ("dummy", m.off(IOVA_BASE))] {
        let mut w = [0u32; 8];
        for (j, x) in w.iter_mut().enumerate() {
            *x = d.r32(base + 4 * j);
        }
        crate::log!("SWEET GMUMEM: {name} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}", w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]);
    }
}
