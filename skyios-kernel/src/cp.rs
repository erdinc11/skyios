// Adreno 618 command processor bring-up, after the GMU has granted GPU access
// (OOB gpu_set acked): GPU memory in the adreno SMMU, SQE microcode,
// ringbuffer, CP_ME_INIT and a first command. Follows Linux a6xx hw_init()
// (research/linux-ref/drivers/gpu/drm/msm/adreno/a6xx_gpu.c), a618 subset.

use crate::xnu::{Dma, Mmio, delay_us};

const SMMU: u64 = 0x0504_0000;
const SMMU_SIZE: usize = 0x10000;
const PAGE: usize = 0x1000;
const CB_BASE: usize = 0x8000;
const CB: usize = 0;
const SMR: usize = 0;
const GPU_SID: u32 = 0;
const VMID: u32 = 1;

// GPU virtual layout, below the 4 GiB TCR limit and away from the UCHE GMEM
// range at 1 MiB
const IOVA_BASE: u64 = 0x0100_0000;
const TABLES: usize = 3 * PAGE;
const SQE_OFF: usize = 0;
const SQE_MAX: usize = 0x8000;
const RING_OFF: usize = SQE_OFF + SQE_MAX;
const RING_BYTES: usize = 0x8000;
const SCRATCH_OFF: usize = RING_OFF + RING_BYTES;
const WINDOW: usize = SCRATCH_OFF + PAGE;

#[cfg(feature = "sqefw")]
static SQE_FW: &[u8] = include_bytes!(env!("SWEET_SQE_FW"));
#[cfg(not(feature = "sqefw"))]
static SQE_FW: &[u8] = &[];

const RBBM_VBIF_CLIENT_QOS_CNTL: usize = 0x0010;
const RBBM_GBIF_HALT: usize = 0x0016;
const RBBM_INT_0_STATUS: usize = 0x0201;
const RBBM_STATUS: usize = 0x0210;
const CP_RB_BASE: usize = 0x0800;
const CP_RB_CNTL: usize = 0x0802;
const CP_RB_RPTR: usize = 0x0806;
const CP_RB_WPTR: usize = 0x0807;
const CP_SQE_CNTL: usize = 0x0808;
const CP_HW_FAULT: usize = 0x0821;
const CP_INTERRUPT_STATUS: usize = 0x0823;
const CP_PROTECT_STATUS: usize = 0x0824;
const CP_STATUS_1: usize = 0x0825;
const CP_SQE_INSTR_BASE: usize = 0x0830;
const CP_ADDR_MODE_CNTL: usize = 0x0842;
const CP_SCRATCH0: usize = 0x0883;
const CP_ROQ_THRESHOLDS_1: usize = 0x08c1;
const CP_ROQ_THRESHOLDS_2: usize = 0x08c2;
const CP_MEM_POOL_SIZE: usize = 0x08c3;
const CP_SQE_STAT_ADDR: usize = 0x0908;
const CP_SQE_STAT_DATA: usize = 0x0909;
const CP_AHB_CNTL: usize = 0x098d;
const GBIF_HALT: usize = 0x3c45;
const RBBM_SECVID_TSB_TRUSTED_BASE: usize = 0xf800;
const RBBM_SECVID_TSB_TRUSTED_SIZE: usize = 0xf802;
const RBBM_SECVID_TSB_CNTL: usize = 0xf803;

// MSM_GPU_RB_CNTL_DEFAULT for a 32 KiB ring and 32-byte blocks, plus NO_UPDATE
// (a618 has no expanded APRIV, so the rptr shadow write-back stays off)
const RB_CNTL: u32 = 12 | (2 << 8) | (1 << 27);

const CP_NOP: u32 = 0x10;
const CP_MEM_WRITE: u32 = 0x3d;
const CP_ME_INIT: u32 = 0x48;

fn parity(v: u32) -> u32 {
    let x = v ^ (v >> 4) ^ (v >> 8) ^ (v >> 12) ^ (v >> 16) ^ (v >> 20) ^ (v >> 24) ^ (v >> 28);
    (0x9669 >> (x & 0xf)) & 1
}

fn pkt4(reg: u32, cnt: u32) -> u32 {
    (4 << 28) | cnt | (parity(cnt) << 7) | ((reg & 0x3ffff) << 8) | (parity(reg) << 27)
}

fn pkt7(op: u32, cnt: u32) -> u32 {
    (7 << 28) | cnt | (parity(cnt) << 15) | ((op & 0x7f) << 16) | (parity(op) << 23)
}

struct Gpu(Mmio);

impl Gpu {
    fn read(&self, reg: usize) -> u32 {
        self.0.read(reg * 4)
    }

    fn write(&self, reg: usize, v: u32) {
        self.0.write(reg * 4, v)
    }

    fn write64(&self, reg: usize, v: u64) {
        self.write(reg, v as u32);
        self.write(reg + 1, (v >> 32) as u32);
    }
}

struct GpuMem {
    dma: Dma,
}

impl GpuMem {
    fn iova(&self, off: usize) -> u64 {
        IOVA_BASE + off as u64
    }

    fn w32(&self, off: usize, v: u32) {
        self.dma.w32(TABLES + off, v)
    }

    fn r32(&self, off: usize) -> u32 {
        self.dma.r32(TABLES + off)
    }
}

fn pte_page(pa: u64) -> u64 {
    pa | (1 << 10) | (1 << 6) | 0b11
}

fn pte_table(pa: u64) -> u64 {
    pa | 0b11
}

// Same context bank programming as gmumem::setup() (cb4, SID 5), here for the
// GPU stream (SID 0) in cb0 with its own VMID.
fn mem_setup() -> Option<GpuMem> {
    let smmu = Mmio::map(SMMU, SMMU_SIZE)?;
    let Some(dma) = Dma::alloc(TABLES + WINDOW) else {
        crate::log!("SWEET CP: contiguous alloc failed");
        return None;
    };
    let iova = IOVA_BASE as usize;
    dma.w64(((iova >> 30) & 0x1ff) * 8, pte_table(dma.pa + PAGE as u64));
    dma.w64(PAGE + ((iova >> 21) & 0x1ff) * 8, pte_table(dma.pa + 2 * PAGE as u64));
    for i in 0..WINDOW / PAGE {
        dma.w64(2 * PAGE + (((iova >> 12) & 0x1ff) + i) * 8, pte_page(dma.pa + (TABLES + i * PAGE) as u64));
    }
    dma.clean();
    let l1 = dma.pa;
    let cb = CB_BASE + CB * PAGE;
    smmu.write(cb, 0);
    smmu.write(PAGE + 0x800 + 4 * CB, 1);
    smmu.write(PAGE + 4 * CB, (1 << 16) | (0xf << 12) | (3 << 8) | VMID);
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
    smmu.write(0x800 + 4 * SMR, (1 << 31) | GPU_SID);
    crate::log!(
        "SWEET CP: {:#x} bytes at pa {:#x}, iova {IOVA_BASE:#x} via cb{CB} (sctlr {:#010x}), smr[{SMR}] {:#010x} s2cr {:#010x}",
        dma.size,
        dma.pa,
        smmu.read(cb),
        smmu.read(0x800 + 4 * SMR),
        smmu.read(0xc00 + 4 * SMR)
    );
    Some(GpuMem { dma })
}

fn smmu_fault() {
    let Some(smmu) = Mmio::map(SMMU, SMMU_SIZE) else {
        return;
    };
    let cb = CB_BASE + CB * PAGE;
    crate::log!(
        "SWEET CP: cb{CB} fsr {:#x} far {:#x} fsynr0 {:#x}; gfsr {:#x} gfsynr1 {:#x}",
        smmu.read(cb + 0x58),
        (smmu.read(cb + 0x64) as u64) << 32 | smmu.read(cb + 0x60) as u64,
        smmu.read(cb + 0x68),
        smmu.read(0x48),
        smmu.read(0x54)
    );
}

// a6xx_ucode_check_version(), a630_sqe.fw: patched (low nibble 0xa) or >= 1.90
fn sqe_ok(words: &[u32]) -> bool {
    words.len() > 3 && ((words[0] & 0xf == 0xa && words[2] & 0xf >= 1) || words[0] & 0xfff >= 0x190)
}

struct Ring<'a> {
    mem: &'a GpuMem,
    wptr: u32,
}

impl Ring<'_> {
    fn emit(&mut self, v: u32) {
        self.mem.w32(RING_OFF + 4 * self.wptr as usize, v);
        self.wptr = (self.wptr + 1) % (RING_BYTES / 4) as u32;
    }

    // a6xx_flush() + a6xx_idle(): publish the ring, then wait for the CP to
    // drain it and for RBBM to report no busy block besides the AHB master
    fn submit(&mut self, gpu: &Gpu, what: &str) -> bool {
        self.mem.dma.clean();
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
        gpu.write(CP_RB_WPTR, self.wptr);
        for i in 0..20_000 {
            let rptr = gpu.read(CP_RB_RPTR);
            if rptr == self.wptr && gpu.read(RBBM_STATUS) & !1 == 0 {
                crate::log!("SWEET CP: {what}: done (rptr {rptr}, {} us)", i * 10);
                return true;
            }
            delay_us(10);
        }
        crate::log!(
            "SWEET CP: {what}: TIMEOUT rptr {} wptr {} status {:#010x} int0 {:#010x} hw_fault {:#010x} cp_int {:#010x} protect {:#010x} status_1 {:#010x}",
            gpu.read(CP_RB_RPTR),
            self.wptr,
            gpu.read(RBBM_STATUS),
            gpu.read(RBBM_INT_0_STATUS),
            gpu.read(CP_HW_FAULT),
            gpu.read(CP_INTERRUPT_STATUS),
            gpu.read(CP_PROTECT_STATUS),
            gpu.read(CP_STATUS_1)
        );
        gpu.write(CP_SQE_STAT_ADDR, 1);
        crate::log!("SWEET CP: sqe pc {:#06x}", gpu.read(CP_SQE_STAT_DATA));
        smmu_fault();
        false
    }
}

pub fn bringup() {
    let Some(gpu) = Mmio::map(crate::gpu::GPU, 0x40000).map(Gpu) else {
        return;
    };
    if SQE_FW.len() <= 4 || SQE_FW.len() - 4 > SQE_MAX || SQE_FW.len() % 4 != 0 {
        crate::log!("SWEET CP: no usable SQE firmware in this kernel ({} bytes)", SQE_FW.len());
        return;
    }
    // adreno_fw_create_bo(): the first dword of the file is not loaded
    let mut head = [0u32; 4];
    for (i, w) in SQE_FW[4..20].chunks_exact(4).enumerate() {
        head[i] = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
    }
    if !sqe_ok(&head) {
        crate::log!("SWEET CP: SQE ucode too old ({:#x}, want >= 0x190)", head[0] & 0xfff);
        return;
    }
    let Some(mem) = mem_setup() else {
        return;
    };
    for (i, w) in SQE_FW[4..].chunks_exact(4).enumerate() {
        mem.w32(SQE_OFF + 4 * i, u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    }
    for i in 0..RING_BYTES / 4 {
        mem.w32(RING_OFF + 4 * i, 0);
    }
    mem.w32(SCRATCH_OFF, 0);
    mem.dma.clean();
    crate::log!("SWEET CP: sqe ucode {} bytes, version {:#x}", SQE_FW.len() - 4, head[0] & 0xfff);

    gpu.write(GBIF_HALT, 0);
    gpu.read(GBIF_HALT);
    gpu.write(RBBM_GBIF_HALT, 0);
    gpu.read(RBBM_GBIF_HALT);
    gpu.write(RBBM_SECVID_TSB_CNTL, 0);
    gpu.write64(RBBM_SECVID_TSB_TRUSTED_BASE, 0);
    gpu.write(RBBM_SECVID_TSB_TRUSTED_SIZE, 0);
    gpu.write(CP_ADDR_MODE_CNTL, 1);
    gpu.write(RBBM_VBIF_CLIENT_QOS_CNTL, 3);
    gpu.write(CP_ROQ_THRESHOLDS_2, 0x0100_00c0);
    gpu.write(CP_ROQ_THRESHOLDS_1, 0x8040_362c);
    gpu.write(CP_MEM_POOL_SIZE, 128);
    gpu.write(CP_AHB_CNTL, 1);

    gpu.write64(CP_SQE_INSTR_BASE, mem.iova(SQE_OFF));
    gpu.write64(CP_RB_BASE, mem.iova(RING_OFF));
    gpu.write(CP_RB_CNTL, RB_CNTL);
    crate::log!(
        "SWEET CP: rb cntl {:#010x} rptr {} wptr {}, starting SQE",
        gpu.read(CP_RB_CNTL),
        gpu.read(CP_RB_RPTR),
        gpu.read(CP_RB_WPTR)
    );
    gpu.write(CP_SQE_CNTL, 1);

    // a6xx_cp_init()
    let mut ring = Ring { mem: &mem, wptr: 0 };
    ring.emit(pkt7(CP_ME_INIT, 8));
    for v in [0x2f, 0x3, 0x2000_0000, 0, 0, 0, 0, 0] {
        ring.emit(v);
    }
    if !ring.submit(&gpu, "CP_ME_INIT") {
        return;
    }

    // first command: one register write and one memory write by the CP
    const REG_MAGIC: u32 = 0x5ca1_ab1e;
    const MEM_MAGIC: u32 = 0xc0ff_ee01;
    ring.emit(pkt4(CP_SCRATCH0 as u32, 1));
    ring.emit(REG_MAGIC);
    let a = mem.iova(SCRATCH_OFF);
    ring.emit(pkt7(CP_MEM_WRITE, 3));
    ring.emit(a as u32);
    ring.emit((a >> 32) as u32);
    ring.emit(MEM_MAGIC);
    ring.emit(pkt7(CP_NOP, 0));
    let ok = ring.submit(&gpu, "first command");
    mem.dma.flush(TABLES + SCRATCH_OFF, 64);
    let (reg, word) = (gpu.read(CP_SCRATCH0), mem.r32(SCRATCH_OFF));
    crate::log!(
        "SWEET CP: scratch reg {reg:#010x} (want {REG_MAGIC:#x}), memory {word:#010x} (want {MEM_MAGIC:#x}){}",
        if ok && reg == REG_MAGIC && word == MEM_MAGIC { ": FIRST GPU COMMAND EXECUTED" } else { "" }
    );
    if word != MEM_MAGIC {
        smmu_fault();
    }
}
