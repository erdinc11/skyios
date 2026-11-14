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

    fn write(&self, reg: usize, v: u32) {
        self.0.write(reg * 4 - GMU_MMIO_OFFSET, v)
    }
}

const GMU_CM3_ITCM_START: usize = 0x1b400;
const GMU_CM3_DTCM_START: usize = 0x1c400;
const GMU_CM3_SYSRESET: usize = 0x1f800;
const GMU_CM3_BOOT_CONFIG: usize = 0x1f801;
const GMU_CM3_FW_INIT_RESULT: usize = 0x1f81c;
const GMU_CM3_CFG: usize = 0x1f82d;
const GMU_GENERAL_7: usize = 0x1f9cc;
const GMU_HFI_SFR_ADDR: usize = 0x1f982;
const GMU_AHB_FENCE_RANGE_0: usize = 0x23b11;
const GMU_SYS_BUS_CONFIG: usize = 0x1f40f;
const GMU_ICACHE_CONFIG: usize = 0x1f400;
const GMU_DCACHE_CONFIG: usize = 0x1f401;
const GMU_PWR_COL_INTER_FRAME_CTRL: usize = 0x1f8c0;
const GMU_PWR_COL_CP_RESP: usize = 0x1f901;
const GMU_CORE_FW_VERSION: usize = 0x1d3f8;
const GMU_CHIPID: u32 = 0x0601_8000;
const GMU_HFI_QTBL_INFO: usize = 0x1f984;
const GMU_HFI_QTBL_ADDR: usize = 0x1f985;
const GMU_HFI_CTRL_INIT: usize = 0x1f986;
const GMU_HFI_CTRL_STATUS: usize = 0x1f980;
const GMU_PWR_COL_CP_MSG: usize = 0x1f900;

#[cfg(feature = "gmufw")]
static GMU_FW: &[u8] = include_bytes!(env!("SWEET_GMU_FW"));
#[cfg(not(feature = "gmufw"))]
static GMU_FW: &[u8] = &[];

fn gmu_boot(gmu: &Gmu, mem: Option<&crate::gmumem::GmuMem>) -> bool {
    if GMU_FW.is_empty() || GMU_FW.len() > 0x8000 || GMU_FW.len() % 4 != 0 {
        crate::log!("SWEET GPU: no usable GMU firmware in this kernel ({} bytes)", GMU_FW.len());
        return false;
    }
    let warm = gmu.read(GMU_GENERAL_7) == 1;
    gmu.write(GMU_GENERAL_7, 1);
    for (i, w) in GMU_FW.chunks_exact(4).enumerate() {
        gmu.write(GMU_CM3_ITCM_START + i, u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    }
    let first = u32::from_le_bytes([GMU_FW[0], GMU_FW[1], GMU_FW[2], GMU_FW[3]]);
    crate::log!(
        "SWEET GPU: gmu fw {} bytes into ITCM ({} boot), itcm[0] = {:#010x} (want {first:#010x})",
        GMU_FW.len(),
        if warm { "warm" } else { "cold" },
        gmu.read(GMU_CM3_ITCM_START)
    );
    gmu.write(GMU_CM3_FW_INIT_RESULT, 0);
    gmu.write(GMU_CM3_BOOT_CONFIG, 0x02);
    gmu.write(GMU_AHB_FENCE_RANGE_0, (1 << 31) | (0xa << 18) | 0xa0);
    gmu.write(GMU_CM3_CFG, 0x4052);
    gmu.write(GMU_HFI_SFR_ADDR, GMU_CHIPID);
    if mem.is_some() {
        gmu.write(GMU_HFI_QTBL_ADDR, crate::gmumem::HFI_IOVA);
        gmu.write(GMU_HFI_QTBL_INFO, 1);
        gmu.write(GMU_PWR_COL_CP_MSG, crate::gmumem::LOG_IOVA | (crate::gmumem::REGION_SIZE / 0x1000 - 1) as u32);
    }
    gmu.write(GMU_SYS_BUS_CONFIG, 1);
    gmu.write(GMU_ICACHE_CONFIG, 1);
    gmu.write(GMU_DCACHE_CONFIG, 1);
    gmu.write(GMU_PWR_COL_INTER_FRAME_CTRL, 0x9c4_0400);
    let dtcm = gmu.read(GMU_CM3_DTCM_START + 0xff8);
    let (mask, want) = if dtcm <= 0x2001_0004 { (u32::MAX, 0xbabe_face) } else { (0x1ff, 0x100) };
    gmu.write(GMU_CM3_SYSRESET, 1);
    gmu.write(GMU_PWR_COL_CP_RESP, 0);
    gmu.write(GMU_CM3_SYSRESET, 0);
    let mut result = 0;
    let mut ok = false;
    for _ in 0..1000 {
        result = gmu.read(GMU_CM3_FW_INIT_RESULT);
        if result & mask == want {
            ok = true;
            break;
        }
        delay_us(100);
    }
    crate::log!(
        "SWEET GPU: gmu start: init result {result:#010x} (want {want:#x} mask {mask:#x}, dtcm[0xff8] {dtcm:#010x}) {}",
        if ok { "OK" } else { "TIMEOUT" }
    );
    crate::log!("SWEET GPU: gmu core fw version {:#010x}", gmu.read(GMU_CORE_FW_VERSION));
    crate::log!("SWEET GPU: gmu2host intr info right after start {:#010x}", gmu.read(0x1f992));
    delay_us(20000);
    crate::log!("SWEET GPU: gmu2host intr info 20 ms later {:#010x}", gmu.read(0x1f992));
    for (name, reg) in GMU_REGS {
        crate::log!("SWEET GPU: gmu {name} = {:#010x}", gmu.read(reg));
    }
    if ok {
        crate::log!("SWEET GPU: stage B done (GMU firmware running)");
    }
    ok
}

const RSCC_BASE: usize = 0x23000;
const GPU: u64 = 0x0500_0000;
const PDC: u64 = 0x0b29_0000;
const PDC_SEQ: u64 = 0x0b49_0000;

const GMU_RSCC_CONTROL_REQ: usize = 0x23b07;
const GMU_RSCC_CONTROL_ACK: usize = 0x23b08;
const GMU_BOOT_SLUMBER_OPTION: usize = 0x1cbf8;
const GMU_GX_VOTE_IDX: usize = 0x1cbf9;
const GMU_MX_VOTE_IDX: usize = 0x1cbfa;
const GMU_HOST2GMU_INTR_SET: usize = 0x1f994;
const GMU_GMU2HOST_INTR_CLR: usize = 0x1f991;
const GMU_GMU2HOST_INTR_INFO: usize = 0x1f992;
const GMU_GX_SPTPRAC_POWER_CONTROL: usize = 0x1a881;
const GMU_SPTPRAC_PWR_CLK_STATUS: usize = 0x1f8d0;
const OOB_BOOT_SLUMBER_SET: u32 = 22;
const OOB_BOOT_SLUMBER_ACK: u32 = 30;

const RSC_STATUS0_DRV0: usize = 0x004;
const RSC_PDC_SEQ_START_ADDR: usize = 0x008;
const RSC_PDC_MATCH_VALUE_LO: usize = 0x009;
const RSC_PDC_MATCH_VALUE_HI: usize = 0x00a;
const RSC_PDC_SLAVE_ID_DRV0: usize = 0x00b;
const RSC_HIDDEN_TCS_CMD0_ADDR: usize = 0x00d;
const RSC_HIDDEN_TCS_CMD0_DATA: usize = 0x00e;
const RSC_OVERRIDE_START_ADDR: usize = 0x100;
const RSC_SEQ_BUSY_DRV0: usize = 0x101;
const RSC_SEQ_MEM_0_DRV0: usize = 0x180;
const RSC_SEQ: [u32; 5] = [0xa7a5_06a0, 0xa1e6_a6e7, 0xa2e0_81e1, 0xe9a9_82e2, 0x0020_e8a8];

const PDC_ENABLE: usize = 0x1140;
const PDC_SEQ_START_ADDR: usize = 0x1148;
const PDC_TCS1: usize = 0x1572;
const PDC_TCS3: usize = 0x15d6;
const PDC_SEQ_MEM: [u32; 5] = [0xfebe_a1e1, 0xa5a4_a3a2, 0x8382_a6e0, 0xbce3_e284, 0x0020_81fc];

impl Gmu {
    fn rsc_read(&self, reg: usize) -> u32 {
        self.0.read(RSCC_BASE + reg * 4)
    }

    fn rsc_write(&self, reg: usize, v: u32) {
        self.0.write(RSCC_BASE + reg * 4, v)
    }

    fn poll(&self, reg: usize, mask: u32, want: u32, tries: u32) -> Option<u32> {
        for _ in 0..tries {
            let v = self.read(reg);
            if v & mask == want {
                return Some(v);
            }
            delay_us(10);
        }
        None
    }
}

fn dw(m: &Mmio, reg: usize, v: u32) {
    m.write(reg * 4, v)
}

fn pdc_tcs(pdc: &Mmio, tcs: usize, gfx: u32, data: [u32; 3]) {
    dw(pdc, tcs + 1, 7);
    dw(pdc, tcs + 2, 0);
    dw(pdc, tcs, 0);
    for (i, (addr, d)) in [0x30010, 0x30000, gfx].into_iter().zip(data).enumerate() {
        dw(pdc, tcs + 3 + 4 * i, 0x10108);
        dw(pdc, tcs + 4 + 4 * i, addr);
        dw(pdc, tcs + 5 + 4 * i, d);
    }
}

fn rpmh_init(gmu: &Gmu) -> bool {
    let Some(gfx) = crate::cmddb::CmdDb::open().and_then(|db| db.addr("gfx.lvl")) else {
        crate::log!("SWEET GPU: gfx.lvl address not in cmd-db");
        return false;
    };
    let (Some(pdc), Some(seq)) = (Mmio::map(PDC, 0x10000), Mmio::map(PDC_SEQ, 0x1000)) else {
        crate::log!("SWEET GPU: pdc map failed");
        return false;
    };
    gmu.rsc_write(RSC_STATUS0_DRV0, 1 << 24);
    gmu.rsc_write(RSC_PDC_SLAVE_ID_DRV0, 1);
    for i in [0, 2, 4] {
        gmu.rsc_write(RSC_HIDDEN_TCS_CMD0_DATA + i, if i == 4 { 0x8000_0000 } else { 0 });
        gmu.rsc_write(RSC_HIDDEN_TCS_CMD0_ADDR + i, 0);
    }
    gmu.rsc_write(RSC_OVERRIDE_START_ADDR, 0);
    gmu.rsc_write(RSC_PDC_SEQ_START_ADDR, 0x4520);
    gmu.rsc_write(RSC_PDC_MATCH_VALUE_LO, 0x4510);
    gmu.rsc_write(RSC_PDC_MATCH_VALUE_HI, 0x4514);
    for (i, w) in RSC_SEQ.into_iter().enumerate() {
        gmu.rsc_write(RSC_SEQ_MEM_0_DRV0 + i, w);
    }
    for (i, w) in PDC_SEQ_MEM.into_iter().enumerate() {
        dw(&seq, i, w);
    }
    pdc_tcs(&pdc, PDC_TCS1, gfx, [1, 0, 0]);
    pdc_tcs(&pdc, PDC_TCS3, gfx, [2, 2, 3]);
    dw(&pdc, PDC_SEQ_START_ADDR, 0);
    dw(&pdc, PDC_ENABLE, 0x8000_0001);
    crate::log!(
        "SWEET GPU: rsc/pdc programmed (gfx.lvl at {gfx:#x}), rsc seq[0] {:#010x}, pdc enable {:#010x}",
        gmu.rsc_read(RSC_SEQ_MEM_0_DRV0),
        pdc.read(PDC_ENABLE * 4)
    );
    true
}

#[allow(dead_code)]
fn rpmh_start(gmu: &Gmu) -> bool {
    gmu.write(GMU_RSCC_CONTROL_REQ, 1 << 1);
    let ack = gmu.poll(GMU_RSCC_CONTROL_ACK, 1 << 1, 1 << 1, 1000);
    let mut busy = 0;
    let mut idle = false;
    for _ in 0..1000 {
        busy = gmu.rsc_read(RSC_SEQ_BUSY_DRV0);
        if busy == 0 {
            idle = true;
            break;
        }
        delay_us(10);
    }
    gmu.write(GMU_RSCC_CONTROL_REQ, 0);
    crate::log!(
        "SWEET GPU: rsc wake: ack {} seq busy {busy:#x}{}",
        if ack.is_some() { "OK" } else { "TIMEOUT" },
        if idle { "" } else { " (stuck)" }
    );
    ack.is_some() && idle
}

fn arc_vote(db: &crate::cmddb::CmdDb) -> Option<(u32, u32)> {
    let mut gfx = [0u16; 16];
    let mut mx = [0u16; 16];
    let ng = db.aux("gfx.lvl", &mut gfx);
    let nm = db.aux("mx.lvl", &mut mx);
    if ng == 0 || nm == 0 {
        return None;
    }
    let gi = ng - 1;
    let mi = mx[..nm].iter().position(|&l| l >= gfx[gi]).unwrap_or(nm - 1);
    Some((gi as u32, mi as u32))
}

fn gfx_rail_on(gmu: &Gmu, gx: u32, mx: u32) -> bool {
    gmu.write(GMU_BOOT_SLUMBER_OPTION, 0);
    gmu.write(GMU_GX_VOTE_IDX, gx);
    gmu.write(GMU_MX_VOTE_IDX, mx);
    gmu.write(GMU_HOST2GMU_INTR_SET, 1 << OOB_BOOT_SLUMBER_SET);
    let ack = gmu.poll(GMU_GMU2HOST_INTR_INFO, 1 << OOB_BOOT_SLUMBER_ACK, 1 << OOB_BOOT_SLUMBER_ACK, 10000);
    let info = gmu.read(GMU_GMU2HOST_INTR_INFO);
    gmu.write(GMU_GMU2HOST_INTR_CLR, 1 << OOB_BOOT_SLUMBER_ACK);
    crate::log!(
        "SWEET GPU: oob boot_slumber (gx idx {gx}, mx idx {mx}): {} intr info {info:#010x}",
        if ack.is_some() { "ACK" } else { "TIMEOUT" }
    );
    ack.is_some()
}

fn sptprac_on(gmu: &Gmu) -> bool {
    gmu.write(GMU_GX_SPTPRAC_POWER_CONTROL, 0x77_8000);
    let r = gmu.poll(GMU_SPTPRAC_PWR_CLK_STATUS, 0x38, 0x28, 1000);
    crate::log!(
        "SWEET GPU: sptprac status {:#010x}{}",
        gmu.read(GMU_SPTPRAC_PWR_CLK_STATUS),
        if r.is_some() { " (on)" } else { " (TIMEOUT)" }
    );
    r.is_some()
}

fn hfi_start(gmu: &Gmu) -> bool {
    gmu.write(GMU_HFI_CTRL_INIT, 1);
    let r = gmu.poll(GMU_HFI_CTRL_STATUS, 1, 1, 10000);
    crate::log!("SWEET GPU: hfi ctrl status {:#x}{}", gmu.read(GMU_HFI_CTRL_STATUS), if r.is_some() { " (HFI up)" } else { " (TIMEOUT)" });
    r.is_some()
}

const DTCM_WORDS: usize = 0x1000;

fn dtcm_snapshot(gmu: &Gmu) -> Option<crate::xnu::Dma> {
    let d = crate::xnu::Dma::alloc(DTCM_WORDS * 4)?;
    for i in 0..DTCM_WORDS {
        d.w32(4 * i, gmu.read(GMU_CM3_DTCM_START + i));
    }
    Some(d)
}

fn dtcm_diff(gmu: &Gmu, before: &crate::xnu::Dma) {
    let mut shown = 0;
    let mut changed = 0;
    for i in 0..DTCM_WORDS {
        let old = before.r32(4 * i);
        let new = gmu.read(GMU_CM3_DTCM_START + i);
        if old != new {
            changed += 1;
            if shown < 64 {
                crate::log!("SWEET GPU: dtcm[{:#06x}] {old:#010x} -> {new:#010x}", 4 * i);
                shown += 1;
            }
        }
    }
    crate::log!("SWEET GPU: dtcm {changed} words changed during OOB");
}

fn stage_c(gmu: &Gmu, cc: &Mmio, mem: Option<&crate::gmumem::GmuMem>) {
    let Some((gx, mx)) = crate::cmddb::CmdDb::open().as_ref().and_then(arc_vote) else {
        crate::log!("SWEET GPU: no arc levels from cmd-db");
        return;
    };
    let gx_before = cc.read(GPUCC_GX_GDSCR);
    let snap = dtcm_snapshot(gmu);
    if !gfx_rail_on(gmu, gx, mx) {
        delay_us(10000);
        if let Some(b) = snap.as_ref() {
            dtcm_diff(gmu, b);
        }
        if let Some(m) = mem {
            crate::gmumem::fault_report();
            crate::gmumem::dump(m);
        }
        return;
    }
    let sp = sptprac_on(gmu);
    if mem.is_some() && hfi_start(gmu) {
        crate::log!("SWEET GPU: stage D done (GMU memory mapped, HFI started)");
    }
    crate::log!("SWEET GPU: gpucc gx_gdscr {gx_before:#010x} -> {:#010x}", cc.read(GPUCC_GX_GDSCR));
    if !sp || cc.read(GPUCC_GX_GDSCR) & GDSC_PWR_ON == 0 {
        crate::log!("SWEET GPU: GX not powered, skipping GPU register read");
        return;
    }
    let Some(gpu) = Mmio::map(GPU, 0x40000) else {
        return;
    };
    crate::log!("SWEET GPU: rbbm_status {:#010x}", gpu.read(0x210 * 4));
    crate::log!("SWEET GPU: stage C done (GX rail on, GPU registers readable)");
}

pub fn bringup(stage: u32) {
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
    if stage >= 3 && !rpmh_init(&gmu) {
        return;
    }
    let mem = if stage >= 4 { crate::gmumem::setup() } else { None };
    if let Some(m) = mem.as_ref() {
        crate::gmumem::hfi_init(m);
    }
    if stage >= 2 && gmu_boot(&gmu, mem.as_ref()) && stage >= 3 {
        stage_c(&gmu, &cc, mem.as_ref());
    }
}
