use crate::xnu::{Mmio, delay_us};

const GCC: u64 = 0x0010_0000;
const GPUCC: u64 = 0x0509_0000;

const GCC_GPU_CFG_AHB_CBCR: usize = 0x71004;
const GCC_GPU_MEMNOC_GFX_CBCR: usize = 0x7100c;
const GCC_GPU_SNOC_DVM_GFX_CBCR: usize = 0x71018;

const CBCR_OFF: u32 = 1 << 31;
const CBCR_EN: u32 = 1 << 0;

const GPUCC_REGS: [(&str, usize); 8] = [
    ("gx_gdscr", 0x100c),
    ("cx_gdscr", 0x106c),
    ("ahb_cbcr", 0x1078),
    ("cx_gmu_cbcr", 0x1098),
    ("cxo_cbcr", 0x109c),
    ("cx_snoc_dvm_cbcr", 0x108c),
    ("gx_gfx3d_cbcr", 0x1054),
    ("gmu_cmd_rcgr", 0x1120),
];

fn cbcr_on(gcc: &Mmio, name: &str, off: usize) -> bool {
    let v = gcc.read(off);
    if v & CBCR_OFF == 0 {
        crate::log!("SWEET GPU: gcc {name} on ({v:#010x})");
        return true;
    }
    gcc.write(off, v | CBCR_EN);
    for _ in 0..200 {
        if gcc.read(off) & CBCR_OFF == 0 {
            crate::log!("SWEET GPU: gcc {name} enabled ({v:#010x} -> {:#010x})", gcc.read(off));
            return true;
        }
        delay_us(10);
    }
    crate::log!("SWEET GPU: gcc {name} stays off ({:#010x})", gcc.read(off));
    false
}

pub fn probe() {
    let Some(gcc) = Mmio::map(GCC, 0x80000) else {
        crate::log!("SWEET GPU: gcc map failed");
        return;
    };
    let ahb = cbcr_on(&gcc, "gpu_cfg_ahb", GCC_GPU_CFG_AHB_CBCR);
    for (name, off) in [("gpu_memnoc_gfx", GCC_GPU_MEMNOC_GFX_CBCR), ("gpu_snoc_dvm_gfx", GCC_GPU_SNOC_DVM_GFX_CBCR)] {
        let v = gcc.read(off);
        crate::log!("SWEET GPU: gcc {name} {v:#010x}{}", if v & CBCR_OFF != 0 { " (off)" } else { "" });
    }
    if !ahb {
        crate::log!("SWEET GPU: no AHB clock to GPUCC, not touching it");
        return;
    }
    let Some(cc) = Mmio::map(GPUCC, 0x9000) else {
        crate::log!("SWEET GPU: gpucc map failed");
        return;
    };
    for (name, off) in GPUCC_REGS {
        crate::log!("SWEET GPU: gpucc {name} @{off:#06x} = {:#010x}", cc.read(off));
    }
}
