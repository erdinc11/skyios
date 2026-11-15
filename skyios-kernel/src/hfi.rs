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
        let mut body = [0u32; 2 + 2 * 16 + 2 * 4];
        let mut gx = [0u16; 16];
        let mut mx = [0u16; 16];
        let mut cx = [0u16; 16];
        let Some(db) = crate::cmddb::CmdDb::open() else {
            return Err(-4);
        };
        let ng = db.aux("gfx.lvl", &mut gx);
        let nm = db.aux("cx.lvl", &mut cx);
        db.aux("mx.lvl", &mut mx);
        if ng > 16 || nm > 4 || ng == 0 || nm == 0 {
            crate::log!("SWEET HFI: perf table level count out of range (gfx {ng}, cx {nm})");
            return Err(-3);
        }
        body[0] = ng as u32;
        body[1] = nm as u32;
        let mut off = 2;
        for (i, v) in gx[..ng].iter().enumerate() {
            body[off + 2 * i] = *v as u32;
            body[off + 2 * i + 1] = FREQ_MHZ[0][i];
        }
        off += 2 * 16;
        for (i, v) in cx[..nm].iter().enumerate() {
            body[off + 2 * i] = *v as u32;
            body[off + 2 * i + 1] = FREQ_MHZ[1][i];
        }
        crate::log!(
            "SWEET HFI: perf table gx {} levels {:?} / {:?} MHz, cx {} levels {:?} / {:?} MHz",
            ng,
            &gx[..ng],
            &FREQ_MHZ[0][..ng],
            nm,
            &cx[..nm],
            &FREQ_MHZ[1][..nm]
        );
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
        let steps: [(&str, fn(&mut Ring, &Gmu, u32) -> Result<(), i32>); 6] = [
            ("init", |r, g, b| r.send_init(g, b)),
            ("fw_version", |r, g, _| r.send_fw_version(g)),
            ("perf_table", |r, g, _| r.send_perf_table(g)),
            ("bw_table", |r, g, _| r.send_bw_table(g)),
            ("test", |r, g, _| r.send_test(g)),
            ("start", |r, g, _| r.send_start(g)),
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

const FREQ_MHZ: [[u32; 16]; 2] = [
    [980, 850, 745, 700, 550, 435, 355, 260, 180, 0, 0, 0, 0, 0, 0, 0],
    [500, 200, 150, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];


