use crate::xnu::{Mmio, delay_us};

const GCC: u64 = 0x0010_0000;
const GPUCC: u64 = 0x0509_0000;
const GMU: u64 = 0x0506_a000;
const GMU_SIZE: usize = 0x31000;
const GMU_MMIO_OFFSET: usize = 0x6a000;

const GCC_GPU_CFG_AHB_CBCR: usize = 0x71004;
const GCC_GPU_MEMNOC_GFX_CBCR: usize = 0x7100c;
const GCC_GPU_SNOC_DVM_GFX_CBCR: usize = 0x71018;
const GCC_GPU_MISC: usize = 0x71028;
const GCC_DDRSS_GPU_AXI_CBCR: usize = 0x4452c;
const GCC_GPLL0_GPU_VOTE: usize = 0x52004;

const GPUCC_GX_GDSCR: usize = 0x100c;
const GPUCC_CX_GDSCR: usize = 0x106c;
const GPUCC_CX_SNOC_DVM_CBCR: usize = 0x108c;
const GPUCC_CX_GMU_CBCR: usize = 0x1098;
const GPUCC_CXO_CBCR: usize = 0x109c;
const GPUCC_GMU_CMD_RCGR: usize = 0x1120;

const CBCR_OFF: u32 = 1 << 31;
const CBCR_EN: u32 = 1 << 0;
const GDSC_PWR_ON: u32 = 1 << 31;
const GDSC_SW_COLLAPSE: u32 = 1 << 0;
const RCG_UPDATE: u32 = 1 << 0;
const RCG_ROOT_OFF: u32 = 1 << 31;

const GMU_REGS: [(&str, usize); 8] = [
    ("cm3_fw_init_result", 0x1f81c),
    ("cm3_fw_busy", 0x1f81a),
    ("cm3_sysreset", 0x1f800),
    ("hfi_ctrl_status", 0x1f980),
    ("sptprac_pwr_clk_status", 0x1f8d0),
    ("cx_gmu_rpmh_power_state", 0x1f8ec),
    ("ao_host_interrupt_status", 0x23b05),
    ("ao_gpu_cx_busy_status", 0x23b0c),
];

fn wait(m: &Mmio, off: usize, mask: u32, want: u32, tries: u32) -> bool {
    for _ in 0..tries {
        if m.read(off) & mask == want {
            return true;
        }
        delay_us(10);
    }
    false
}

fn branch_on(m: &Mmio, unit: &str, name: &str, off: usize, extra: u32) -> bool {
    let v = m.read(off);
    m.write(off, v | CBCR_EN | extra);
    let ok = wait(m, off, CBCR_OFF, 0, 500);
    crate::log!("SWEET GPU: {unit} {name} {v:#010x} -> {:#010x}{}", m.read(off), if ok { "" } else { " (still off)" });
    ok
}

fn gdsc_on(cc: &Mmio, name: &str, off: usize) -> bool {
    let v = cc.read(off);
    if v & GDSC_PWR_ON == 0 || v & GDSC_SW_COLLAPSE != 0 {
        cc.write(off, v & !GDSC_SW_COLLAPSE);
    }
    let ok = wait(cc, off, GDSC_PWR_ON, GDSC_PWR_ON, 2000);
    crate::log!("SWEET GPU: gdsc {name} {v:#010x} -> {:#010x}{}", cc.read(off), if ok { "" } else { " (no power)" });
    ok
}

fn gmu_clk_200mhz(cc: &Mmio) -> bool {
    let cmd = GPUCC_GMU_CMD_RCGR;
    let before = cc.read(cmd + 4);
    cc.write(cmd + 4, (6 << 8) | 2);
    cc.write(cmd, cc.read(cmd) | RCG_UPDATE);
    let ok = wait(cc, cmd, RCG_UPDATE, 0, 500);
    crate::log!(
        "SWEET GPU: gmu rcg cfg {before:#010x} -> {:#010x} cmd {:#010x}{}",
        cc.read(cmd + 4),
        cc.read(cmd),
        if ok { "" } else { " (update stuck)" }
    );
    ok
}

struct Gmu(Mmio);

impl Gmu {
    fn read(&self, reg: usize) -> u32 {
        self.0.read(reg * 4 - GMU_MMIO_OFFSET)
    }
}

pub fn bringup() {
    let (Some(gcc), Some(cc)) = (Mmio::map(GCC, 0x80000), Mmio::map(GPUCC, 0x9000)) else {
        crate::log!("SWEET GPU: map failed");
        return;
    };
    if !branch_on(&gcc, "gcc", "gpu_cfg_ahb", GCC_GPU_CFG_AHB_CBCR, 0) {
        return;
    }
    let vote = gcc.read(GCC_GPLL0_GPU_VOTE);
    gcc.write(GCC_GPLL0_GPU_VOTE, vote | (1 << 15) | (1 << 16));
    let misc = gcc.read(GCC_GPU_MISC);
    gcc.write(GCC_GPU_MISC, misc | 3);
    crate::log!("SWEET GPU: gcc gpll0 vote {vote:#010x} -> {:#010x}, misc {misc:#010x} -> {:#010x}", gcc.read(GCC_GPLL0_GPU_VOTE), gcc.read(GCC_GPU_MISC));
    branch_on(&gcc, "gcc", "gpu_memnoc_gfx", GCC_GPU_MEMNOC_GFX_CBCR, 0);
    branch_on(&gcc, "gcc", "gpu_snoc_dvm_gfx", GCC_GPU_SNOC_DVM_GFX_CBCR, 0);
    branch_on(&gcc, "gcc", "ddrss_gpu_axi", GCC_DDRSS_GPU_AXI_CBCR, 0);
    crate::log!("SWEET GPU: gpucc gx_gdscr {:#010x} (left alone)", cc.read(GPUCC_GX_GDSCR));
    if !gdsc_on(&cc, "cx", GPUCC_CX_GDSCR) {
        return;
    }
    branch_on(&cc, "gpucc", "cxo", GPUCC_CXO_CBCR, 0);
    if !gmu_clk_200mhz(&cc) {
        return;
    }
    if !branch_on(&cc, "gpucc", "cx_gmu", GPUCC_CX_GMU_CBCR, (0xf << 8) | (0xf << 4)) {
        return;
    }
    branch_on(&cc, "gpucc", "cx_snoc_dvm", GPUCC_CX_SNOC_DVM_CBCR, 0);
    let Some(gmu) = Mmio::map(GMU, GMU_SIZE).map(Gmu) else {
        crate::log!("SWEET GPU: gmu map failed");
        return;
    };
    for (name, reg) in GMU_REGS {
        crate::log!("SWEET GPU: gmu {name} = {:#010x}", gmu.read(reg));
    }
    crate::log!("SWEET GPU: gmu itcm[0..2] = {:#010x} {:#010x}", gmu.read(0x1b400), gmu.read(0x1b401));
    crate::log!("SWEET GPU: stage A done (CX up, GMU registers readable)");
}
