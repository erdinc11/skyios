use crate::gpu::Gmu;
use crate::gmumem::{GmuMem, HFI_IOVA};
use crate::xnu::delay_us;

const CMD_QUEUE: usize = 0;
const RESP_QUEUE: usize = 1;

const QTBL_OFF: u32 = 0;
const QHDR0_OFF: u32 = 24;
const QHDR_SIZE: u32 = 48;
const NR_QUEUES: u32 = 3;
const Q_DATA_DWORDS: u32 = 0x1000 / 4;

const MSG_CMD: u32 = 0;
const F2H_ACK: u32 = 126;
const F2H_ERROR: u32 = 100;

const H2F_INIT: u32 = 0;
const H2F_FW_VERSION: u32 = 1;
const H2F_BW_TABLE: u32 = 3;
const H2F_PERF_TABLE: u32 = 4;
const H2F_TEST: u32 = 5;
const H2F_START: u32 = 10;
const H2F_GX_BW_PERF_VOTE: u32 = 12;

const MAX_PAYLOAD: usize = 16 + 4 * 101;

pub struct Ring {
    mem: GmuMem,
    wptr: [u32; NR_QUEUES as usize],
    rptr: [u32; NR_QUEUES as usize],
    seq: u32,
}

impl Ring {
    pub fn new(mem: GmuMem) -> Ring {
        Ring { mem, wptr: [0; 3], rptr: [0; 3], seq: 0 }
    }

    fn hdr_off(&self, q: usize) -> usize {
        self.mem.off(HFI_IOVA + QHDR0_OFF + q as u32 * QHDR_SIZE)
    }

    fn data_off(&self, q: usize) -> usize {
        self.mem.off(HFI_IOVA + 0x1000 * (q as u32 + 1))
    }

    fn hdr_r(&self, q: usize, dw: usize) -> u32 {
        self.mem.dma.r32(self.hdr_off(q) + 4 * dw)
    }

    fn hdr_w(&self, q: usize, dw: usize, v: u32) {
        self.mem.dma.w32(self.hdr_off(q) + 4 * dw, v);
    }

    pub fn init_queues(&mut self) {
        let d = &self.mem.dma;
        let table = self.mem.off(HFI_IOVA);
        let tbl_size = QTBL_OFF + QHDR0_OFF + NR_QUEUES * QHDR_SIZE;
        d.w32(table, 0);
        d.w32(table + 4, tbl_size / 4);
        d.w32(table + 8, QHDR0_OFF / 4);
        d.w32(table + 12, QHDR_SIZE / 4);
        d.w32(table + 16, NR_QUEUES);
        d.w32(table + 20, NR_QUEUES);
        for q in 0..NR_QUEUES as usize {
            let id = match q {
                CMD_QUEUE => 0,
                RESP_QUEUE => 4,
                _ => 5,
            };
            let vals = [
                1,
                HFI_IOVA + 0x1000 * (q as u32 + 1),
                (10 << 8) | id,
                Q_DATA_DWORDS,
                0,
                0,
                1,
                1,
                1,
                0,
                0,
                0,
            ];
            for (j, v) in vals.into_iter().enumerate() {
                self.hdr_w(q, j, v);
            }
            let base = self.data_off(q);
            for i in 0..Q_DATA_DWORDS as usize {
                d.w32(base + 4 * i, 0);
            }
            self.wptr[q] = 0;
            self.rptr[q] = 0;
        }
        d.clean();
        self.seq = 0;
    }

    fn queue_write(&mut self, q: usize, data: &[u32]) {
        let mut idx = self.wptr[q];
        for &w in data {
            self.mem.dma.w32(self.data_off(q) + 4 * idx as usize, w);
            idx = (idx + 1) % Q_DATA_DWORDS;
        }
        self.wptr[q] = idx;
        self.mem.dma.clean();
        self.hdr_w(q, 11, idx);
        self.mem.dma.clean();
    }

    fn queue_read(&mut self, q: usize, out: &mut [u32]) -> usize {
        // the GMU writes the queue behind the CPU cache: drop stale lines first
        self.mem.dma.flush(self.hdr_off(q), QHDR_SIZE as usize);
        self.mem.dma.flush(self.data_off(q), 4 * Q_DATA_DWORDS as usize);
        let wi = self.hdr_r(q, 11);
        if self.rptr[q] == wi {
            return 0;
        }
        let mut idx = self.rptr[q];
        let hdr = self.mem.dma.r32(self.data_off(q) + 4 * idx as usize);
        let size = ((hdr >> 8) & 0xff) as usize;
        let n = size.min(out.len());
        for item in out.iter_mut().take(n) {
            *item = self.mem.dma.r32(self.data_off(q) + 4 * idx as usize);
            idx = (idx + 1) % Q_DATA_DWORDS;
        }
        self.rptr[q] = idx;
        self.mem.dma.clean();
        self.hdr_w(q, 10, idx);
        self.mem.dma.clean();
        n
    }

    fn dump_queues(&self, tag: &str) {
        for q in 0..NR_QUEUES as usize {
            let base = self.data_off(q);
            self.mem.dma.flush(self.hdr_off(q), QHDR_SIZE as usize);
            self.mem.dma.flush(base, 32);
            let mut w = [0u32; 8];
            for (j, x) in w.iter_mut().enumerate() {
                *x = self.mem.dma.r32(base + 4 * j);
            }
            crate::log!(
                "SWEET HFI: {tag} q{q} hdr status {} type {:#x} size {} dropped {} rx_req {} tx_req {} rd {} wr {} data {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}",
                self.hdr_r(q, 0),
                self.hdr_r(q, 2),
                self.hdr_r(q, 3),
                self.hdr_r(q, 5),
                self.hdr_r(q, 8),
                self.hdr_r(q, 9),
                self.hdr_r(q, 10),
                self.hdr_r(q, 11),
                w[0],
                w[1],
                w[2],
                w[3],
                w[4],
                w[5],
                w[6],
                w[7]
            );
        }
    }

    fn dump_log(&self) {
        let base = self.mem.off(crate::gmumem::LOG_IOVA);
        for i in 0..4 {
            let mut w = [0u32; 8];
            for (j, x) in w.iter_mut().enumerate() {
                *x = self.mem.dma.r32(base + 32 * i + 4 * j);
            }
            crate::log!(
                "SWEET HFI: log[{i}] {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}",
                w[0],
                w[1],
                w[2],
                w[3],
                w[4],
                w[5],
                w[6],
                w[7]
            );
        }
    }

    fn wait_msg(&mut self, gmu: &Gmu, id: u32, seq: u32) -> bool {
        for _ in 0..20_000 {
            let v = gmu.read(0x1f992);
            if v & 1 != 0 {
                gmu.write(0x1f991, 1);
                return true;
            }
            if v & (1 << 23) != 0 {
                crate::log!("SWEET HFI: msg {id} seq {seq}: CM3 fault while waiting, intr {v:#010x}");
                return false;
            }
            delay_us(50);
        }
        crate::log!("SWEET HFI: msg {id} seq {seq}: response interrupt TIMEOUT, intr {:#010x}", gmu.read(0x1f992));
        false
    }

    fn wait_ack(&mut self, gmu: &Gmu, id: u32, seq: u32, out: &mut [u32]) -> Result<(), i32> {
        if !self.wait_msg(gmu, id, seq) {
            return Err(-1);
        }
        let mut resp = [0u32; MAX_PAYLOAD];
        for _ in 0..8 {
            let n = self.queue_read(RESP_QUEUE, &mut resp);
            if n == 0 {
                crate::log!("SWEET HFI: msg {id} seq {seq}: response queue empty");
                if !self.wait_msg(gmu, id, seq) {
                    return Err(-1);
                }
                continue;
            }
            let hdr = resp[0];
            let rid = hdr & 0xff;
            let rseq = (hdr >> 20) & 0xfff;
            if rid == F2H_ERROR {
                crate::log!("SWEET HFI: msg {id} seq {seq}: GMU firmware error {} ({:#x})", resp[1], resp[1]);
                continue;
            }
            if rid != F2H_ACK {
                crate::log!("SWEET HFI: msg {id} seq {seq}: unexpected response id {rid} seq {rseq}");
                continue;
            }
            let ret = resp[1];
            // an ack carries the acked message's header in resp[1]; its seq is there
            let rseq = (ret >> 20) & 0xfff;
            if rseq != seq {
                crate::log!("SWEET HFI: msg {id} seq {seq}: ack for seq {rseq} (ret hdr {ret:#010x}), ignored");
                continue;
            }
            if resp[2] != 0 {
                crate::log!("SWEET HFI: msg {id} seq {seq}: returned error {}", resp[2]);
                return Err(resp[2] as i32);
            }
            let want = out.len().min(n.saturating_sub(3));
            out[..want].copy_from_slice(&resp[3..3 + want]);
            return Ok(());
        }
        Err(-2)
    }

    fn send(&mut self, gmu: &Gmu, id: u32, body: &[u32], out: &mut [u32]) -> Result<(), i32> {
        self.seq = (self.seq + 1) % 0xfff;
        let seq = self.seq;
        let dwords = (1 + body.len()) as u32;
        let mut msg = [0u32; MAX_PAYLOAD];
        msg[0] = (seq << 20) | (MSG_CMD << 16) | (dwords << 8) | id;
        msg[1..1 + body.len()].copy_from_slice(body);
        self.queue_write(CMD_QUEUE, &msg[..1 + body.len()]);
        gmu.write(0x1f994, 1);
        let r = self.wait_ack(gmu, id, seq, out);
        match r {
            Ok(()) => crate::log!("SWEET HFI: msg {id} seq {seq} acked"),
            Err(e) => crate::log!("SWEET HFI: msg {id} seq {seq} failed ({e})"),
        }
        r
    }

    pub fn send_init(&mut self, gmu: &Gmu, boot: u32) -> Result<(), i32> {
        let dbg = crate::gmumem::DEBUG_IOVA;
        let body = [crate::gpu::GMU_HFI_SEG_ID, dbg, crate::gmumem::REGION_SIZE as u32, boot];
        let mut out = [0u32; 0];
        self.send(gmu, H2F_INIT, &body, &mut out)
    }

    pub fn send_fw_version(&mut self, gmu: &Gmu) -> Result<(), i32> {
        let body = [(1 << 28) | (1 << 19) | (1 << 17)];
        let mut out = [0u32; 16];
        let r = self.send(gmu, H2F_FW_VERSION, &body, &mut out);
        if r.is_ok() {
            crate::log!("SWEET HFI: gmu firmware version {:#010x} (want major 1 minor 10 step 2)", out[0]);
        }
        r
    }

    pub fn send_perf_table(&mut self, gmu: &Gmu) -> Result<(), i32> {
        const GX_SLOTS: usize = 16;
        const CX_SLOTS: usize = 4;
        let mut body = [0u32; 2 + 2 * GX_SLOTS + 2 * CX_SLOTS];
        let Some(db) = crate::cmddb::CmdDb::open() else {
            return Err(-4);
        };
        let bin = gpu_speed_bin();
        let opp = gpu_opp(bin);
        if opp.len() > GX_SLOTS || GMU_OPP.len() > CX_SLOTS {
            return Err(-3);
        }
        let mut gx = [0u32; GX_SLOTS];
        let mut cx = [0u32; CX_SLOTS];
        arc_votes(&db, opp, &mut gx, "gfx.lvl", gx_secondary())?;
        arc_votes(&db, &GMU_OPP, &mut cx, "cx.lvl", "mx.lvl")?;

        body[0] = opp.len() as u32;
        body[1] = GMU_OPP.len() as u32;
        for i in 0..GX_SLOTS {
            body[2 + 2 * i] = gx[i];
            body[3 + 2 * i] = if i < opp.len() { opp[i].0 * 1000 } else { 0 }; // kHz, as Linux sends
        }
        let off = 2 + 2 * GX_SLOTS;
        for i in 0..CX_SLOTS {
            body[off + 2 * i] = cx[i];
            body[off + 2 * i + 1] = if i < GMU_OPP.len() { GMU_OPP[i].0 * 1000 } else { 0 };
        }

        crate::log!("SWEET HFI: perf table speed bin {bin}, {} gpu levels", opp.len());
        for i in 0..opp.len() {
            crate::log!("SWEET HFI:   gx {i}: {} MHz corner {} vote {:#010x}", opp[i].0, opp[i].1, gx[i]);
        }
        for i in 0..GMU_OPP.len() {
            crate::log!(
                "SWEET HFI:   cx {i}: {} MHz corner {} vote {:#010x}",
                GMU_OPP[i].0,
                GMU_OPP[i].1,
                cx[i]
            );
        }
        let mut out = [0u32; 0];
        self.send(gmu, H2F_PERF_TABLE, &body, &mut out)
    }

    /// `a618_build_bw_table()`: a single "off" level, one CNOC vote and three
    /// DDR votes. The struct has 160 dwords including the header, so the body
    /// is 159 dwords.
    pub fn send_bw_table(&mut self, gmu: &Gmu) -> Result<(), i32> {
        const BW_BODY: usize = 159;
        let mut body = [0u32; BW_BODY];

        // Offsets are struct dword offsets minus the header.
        const BW_LEVEL_NUM: usize = 0;
        const CNOC_CMDS_NUM: usize = 1;
        const DDR_CMDS_NUM: usize = 2;
        const CNOC_WAIT_BITMASK: usize = 3;
        const DDR_WAIT_BITMASK: usize = 4;
        const CNOC_ADDRS: usize = 5; // cnoc_cmds_addrs[6]
        const CNOC_DATA: usize = 11; // cnoc_cmds_data[2][6]
        const DDR_ADDRS: usize = 23; // ddr_cmds_addrs[8]
        const DDR_DATA: usize = 31; // ddr_cmds_data[16][8]

        body[BW_LEVEL_NUM] = 1;
        body[CNOC_CMDS_NUM] = 1;
        body[DDR_CMDS_NUM] = 3;
        body[CNOC_WAIT_BITMASK] = 0x01;
        body[DDR_WAIT_BITMASK] = 0x01;

        body[CNOC_ADDRS] = 0x5007c;
        body[CNOC_DATA] = 0x4000_0000;
        body[CNOC_DATA + 6] = 0x6000_0001;

        body[DDR_ADDRS] = 0x50000;
        body[DDR_ADDRS + 1] = 0x5003c;
        body[DDR_ADDRS + 2] = 0x5000c;
        body[DDR_DATA] = 0x4000_0000;
        body[DDR_DATA + 1] = 0x4000_0000;
        body[DDR_DATA + 2] = 0x4000_0000;

        let mut arbs = 0u32;
        for w in body.iter() {
            arbs |= *w;
        }
        crate::log!(
            "SWEET HFI: bw table a618 ({} levels, cnoc {} ddr {} votes, {BW_BODY} dwords, or {arbs:#x})",
            body[BW_LEVEL_NUM],
            body[CNOC_CMDS_NUM],
            body[DDR_CMDS_NUM]
        );
        let mut out = [0u32; 0];
        self.send(gmu, H2F_BW_TABLE, &body, &mut out)
    }

    pub fn send_test(&mut self, gmu: &Gmu) -> Result<(), i32> {
        let mut out = [0u32; 0];
        self.send(gmu, H2F_TEST, &[], &mut out)
    }

    pub fn send_start(&mut self, gmu: &Gmu) -> Result<(), i32> {
        let mut out = [0u32; 0];
        self.send(gmu, H2F_START, &[], &mut out)
    }

    pub fn send_freq(&mut self, gmu: &Gmu, level: u32, bw: u32) -> Result<(), i32> {
        let body = [1, level, bw];
        let mut out = [0u32; 0];
        self.send(gmu, H2F_GX_BW_PERF_VOTE, &body, &mut out)
    }

    pub fn boot(&mut self, gmu: &Gmu, boot_state: u32) -> bool {
        self.init_queues();
        // a6xx_hfi_start_v1(): the legacy (v1) GMU gets no HFI message after
        // TEST until the next boot; START is a v2 message and is never acked here
        let steps: [(&str, fn(&mut Ring, &Gmu, u32) -> Result<(), i32>); 5] = [
            ("init", |r, g, b| r.send_init(g, b)),
            ("fw_version", |r, g, _| r.send_fw_version(g)),
            ("perf_table", |r, g, _| r.send_perf_table(g)),
            ("bw_table", |r, g, _| r.send_bw_table(g)),
            ("test", |r, g, _| r.send_test(g)),
        ];
        for (name, f) in steps {
            if f(self, gmu, boot_state).is_err() {
                crate::log!("SWEET HFI: boot aborted at {name}");
                self.dump_queues(name);
                self.dump_log();
                return false;
            }
            delay_us(2000);
        }
        delay_us(5000);
        self.dump_queues("done");
        self.dump_log();
        if let Some(gpu) = crate::xnu::Mmio::map(crate::gpu::GPU, 0x40000) {
            for _ in 0..20 {
                let s = gpu.read(0x210 * 4);
                if s & 0x00ff_fffc != 0 {
                    crate::log!("SWEET HFI: rbbm_status after start {s:#010x} (gpu blocks busy)");
                    break;
                }
                delay_us(500);
            }
        }
        crate::log!("SWEET HFI: boot sequence done (v1, boot state {boot_state})");
        true
    }
}

const RPMH_LEVEL_MIN_SVS: u16 = 48;
const RPMH_LEVEL_LOW_SVS: u16 = 64;
const RPMH_LEVEL_SVS: u16 = 128;
const RPMH_LEVEL_SVS_L1: u16 = 192;
const RPMH_LEVEL_NOM: u16 = 256;
const RPMH_LEVEL_NOM_L1: u16 = 320;
const RPMH_LEVEL_TURBO: u16 = 384;
const RPMH_LEVEL_TURBO_L1: u16 = 416;

// SM7150 `gpu_opp_table`, ascending, as a6xx_gmu_build_freq_table() builds it
// (`dev_pm_opp_find_freq_ceil` from 1). The second field is the `opp-level`
// RPMh corner. Speed bins and frequencies come from the sweet DT
// (`qcom,gpu-pwrlevel-bins`); the corners are mainline sm7150's.
const GPU_OPP_825: [(u32, u16); 9] = [
    (0, 0),
    (180, RPMH_LEVEL_MIN_SVS),
    (267, RPMH_LEVEL_LOW_SVS),
    (355, RPMH_LEVEL_SVS),
    (430, RPMH_LEVEL_SVS_L1),
    (565, RPMH_LEVEL_NOM),
    (650, RPMH_LEVEL_NOM_L1),
    (800, RPMH_LEVEL_TURBO),
    (825, RPMH_LEVEL_TURBO_L1),
];
const GPU_OPP_800: [(u32, u16); 8] = [
    (0, 0),
    (180, RPMH_LEVEL_MIN_SVS),
    (267, RPMH_LEVEL_LOW_SVS),
    (355, RPMH_LEVEL_SVS),
    (430, RPMH_LEVEL_SVS_L1),
    (565, RPMH_LEVEL_NOM),
    (650, RPMH_LEVEL_NOM_L1),
    (800, RPMH_LEVEL_TURBO),
];
const GPU_OPP_700: [(u32, u16); 8] = [
    (0, 0),
    (180, RPMH_LEVEL_MIN_SVS),
    (267, RPMH_LEVEL_LOW_SVS),
    (355, RPMH_LEVEL_SVS),
    (430, RPMH_LEVEL_SVS_L1),
    (565, RPMH_LEVEL_NOM),
    (650, RPMH_LEVEL_NOM_L1),
    (700, RPMH_LEVEL_TURBO),
];
const GPU_OPP_610: [(u32, u16); 7] = [
    (0, 0),
    (180, RPMH_LEVEL_MIN_SVS),
    (267, RPMH_LEVEL_LOW_SVS),
    (355, RPMH_LEVEL_SVS),
    (430, RPMH_LEVEL_SVS_L1),
    (565, RPMH_LEVEL_NOM),
    (610, RPMH_LEVEL_NOM_L1),
];

const GMU_OPP: [(u32, u16); 2] = [(0, 0), (200, RPMH_LEVEL_MIN_SVS)];

const QFPROM_PA: u64 = 0x0078_0000;
const QFPROM_SIZE: usize = 0x6300;
const GPU_SPEED_BIN_OFF: usize = 0x41a0;
const GPU_SPEED_BIN_MASK: u32 = 0x1fe0_0000;
const GPU_SPEED_BIN_SHIFT: u32 = 21;

// kgsl reads the speed bin fuse from the QFPROM window in its own reg range and
// picks the pwrlevel bin by it; an unknown or unreadable fuse uses bin 0.
fn gpu_speed_bin() -> u32 {
    let Some(fuse) = crate::xnu::Mmio::map(QFPROM_PA, QFPROM_SIZE) else {
        return 0;
    };
    let raw = fuse.read(GPU_SPEED_BIN_OFF);
    let bin = (raw & GPU_SPEED_BIN_MASK) >> GPU_SPEED_BIN_SHIFT;
    match bin {
        128 | 146 | 167 | 172 => bin,
        _ => 0,
    }
}

fn gpu_opp(bin: u32) -> &'static [(u32, u16)] {
    match bin {
        128 => &GPU_OPP_610,
        146 => &GPU_OPP_700,
        167 => &GPU_OPP_800,
        _ => &GPU_OPP_825,
    }
}

/// Perf index of the highest GPU level; HFI_GX_BW_PERF_VOTE and the OOB rail
/// vote use it (`gmu->nr_gpu_freqs - 1`).
pub fn gpu_top_level() -> u32 {
    (gpu_opp(gpu_speed_bin()).len() - 1) as u32
}

/// (GX vote idx, MX vote idx) of the highest GPU level, for the OOB
/// boot_slumber rail vote (`gmu->gx_arc_votes[nr_gpu_freqs - 1]`).
pub fn gx_rail_vote() -> Option<(u32, u32)> {
    let db = crate::cmddb::CmdDb::open()?;
    let opp = gpu_opp(gpu_speed_bin());
    let mut votes = [0u32; 16];
    arc_votes(&db, opp, &mut votes, "gfx.lvl", gx_secondary()).ok()?;
    let v = votes[opp.len() - 1];
    Some((v & 0xff, (v >> 8) & 0xff))
}

fn gx_secondary() -> &'static str {
    let mut gmxc = [0u16; 16];
    match crate::cmddb::CmdDb::open() {
        Some(db) if db.aux("gmxc.lvl", &mut gmxc) > 0 => "gmxc.lvl",
        _ => "mx.lvl",
    }
}

// a6xx_gmu_rpmh_arc_index(): first corner >= level, optionally clamped to the
// highest available corner.
fn arc_index(arc: &[u16], level: u16, clamp: bool) -> Option<usize> {
    if let Some(i) = arc.iter().position(|&a| a >= level) {
        return Some(i);
    }
    if clamp { Some(arc.len() - 1) } else { None }
}

// a6xx_gmu_rpmh_arc_votes_init(): one vote per OPP level, packing the matched
// primary corner, the secondary rail's corner index and the primary index.
fn arc_votes(
    db: &crate::cmddb::CmdDb,
    opp: &[(u32, u16)],
    out: &mut [u32],
    pri_id: &str,
    sec_id: &str,
) -> Result<(), i32> {
    let mut pri = [0u16; 16];
    let mut sec = [0u16; 16];
    let np = db.aux(pri_id, &mut pri);
    let ns = db.aux(sec_id, &mut sec);
    if np == 0 || ns == 0 {
        crate::log!("SWEET HFI: no cmd-db arc levels for {pri_id} ({np}) / {sec_id} ({ns})");
        return Err(-5);
    }
    for (i, &(_, level)) in opp.iter().enumerate() {
        let Some(pi) = arc_index(&pri[..np], level, false) else {
            crate::log!("SWEET HFI: corner {level} not in {pri_id} {:?}", &pri[..np]);
            return Err(-6);
        };
        let Some(si) = arc_index(&sec[..ns], pri[pi], true) else {
            return Err(-6);
        };
        out[i] = ((pri[pi] as u32) << 16) | ((si as u32) << 8) | pi as u32;
    }
    Ok(())
}



