use super::ce::CeEngine;
use super::htc::HtcState;

const WMI_CMD_HDR: usize = 6;
const TLV_HDR: usize = 4;

const CMD_INIT: u32 = 0x1;
const CMD_START_SCAN: u32 = 0x2007;
const CMD_STOP_SCAN: u32 = 0x2008;
const CMD_VDEV_CREATE: u32 = 0x5001;
const CMD_VDEV_DELETE: u32 = 0x5002;
pub const CMD_VDEV_START: u32 = 0x5003;
pub const CMD_VDEV_UP: u32 = 0x5005;
pub const CMD_VDEV_STOP: u32 = 0x5006;
pub const CMD_VDEV_DOWN: u32 = 0x5007;
pub const CMD_VDEV_INSTALL_KEY: u32 = 0x5009;
pub const CMD_PEER_CREATE: u32 = 0x6001;
pub const CMD_PEER_DELETE: u32 = 0x6002;
pub const CMD_PEER_SET_PARAM: u32 = 0x6004;
pub const CMD_PEER_ASSOC: u32 = 0x6005;
pub const CMD_MGMT_TX_SEND: u32 = 0x7008;
pub const CMD_STA_PS_MODE: u32 = 0x9001;

const EV_SERVICE_READY: u32 = 0x1;
const EV_READY: u32 = 0x2;
const EV_SCAN: u32 = 0x200B;
const EV_MGMT_RX: u32 = 0x200C;
const EV_VDEV_START_RESP: u32 = 0x600A;

const TAG_SERVICE_READY: u16 = 0x000B;
const TAG_RESOURCE_CONFIG: u16 = 0x0021;
const TAG_INIT_CMD: u16 = 0x0023;
const TAG_SCAN_CMD_FIXED: u16 = 0x0024;
const TAG_SCAN_CHANNEL: u16 = 0x0025;
const TAG_SCAN_BSSID: u16 = 0x0026;
const TAG_SCAN_SSID: u16 = 0x0027;
const TAG_SCAN_IE: u16 = 0x002A;
const TAG_MAC_ADDR_CMD: u16 = 0x002F;
const TAG_MGMT_RX_HDR: u16 = 0x0032;
const TAG_SCAN_EV: u16 = 0x0038;
const TAG_ARRAY_BYTE: u16 = 0x0101;

const SCAN_FREQS: [u16; 37] = [
    2412, 2417, 2422, 2427, 2432, 2437, 2442, 2447, 2452, 2457, 2462, 2467, 2472, 2484,
    5180, 5200, 5220, 5240, 5260, 5280, 5300, 5320,
    5500, 5520, 5540, 5560, 5580, 5600, 5620, 5640, 5660, 5680, 5700,
    5745, 5765, 5785, 5805,
];

static RES_CFG: [u32; 44] = [
    1, 44, 33, 0, 4, 0, 1056, 0, 544, 1056, 544, 1, 0, 32, 32, 2, 1, 1, 1, 1056, 2,
    0, 0, 0, 0, 0, 0, 0x00000003, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0,
];

pub const OUR_MAC: [u8; 6] = [0x02, 0x53, 0x57, 0x45, 0x45, 0x54];

pub enum WmiEvent<'a> {
    Ready,
    ScanEvent(&'a [u8]),
    MgmtRx { freq: u16, rssi: i8, frame: &'a [u8] },
    VdevStartResp { status: u32 },
    Other(u32),
}

pub struct WmiState {
    pub mac: [u8; 6],
    pub ready: bool,
    svc_ready: bool,
}

impl WmiState {
    pub fn new() -> Self {
        Self { mac: OUR_MAC, ready: false, svc_ready: false }
    }

    pub fn rx<'a>(&mut self, htc: &mut HtcState, ce: &mut CeEngine, p: &'a [u8]) -> Option<WmiEvent<'a>> {
        if p.len() < WMI_CMD_HDR {
            return None;
        }
        let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let body = &p[WMI_CMD_HDR..];
        match eid {
            EV_SERVICE_READY => {
                self.on_svc_ready(htc, ce, body);
                None
            }
            EV_READY => {
                self.on_ready(body);
                Some(WmiEvent::Ready)
            }
            EV_SCAN => {
                Some(WmiEvent::ScanEvent(body))
            }
            EV_MGMT_RX => {
                self.parse_mgmt_rx(body)
            }
            EV_VDEV_START_RESP => {
                let status = if body.len() >= 4 {
                    u32::from_le_bytes([body[0], body[1], body[2], body[3]])
                } else {
                    0xFFFF
                };
                Some(WmiEvent::VdevStartResp { status })
            }
            _ => Some(WmiEvent::Other(eid))
        }
    }

    fn on_svc_ready(&mut self, htc: &mut HtcState, ce: &mut CeEngine, _body: &[u8]) {
        crate::log!("SWEET WLAN: wmi: SERVICE_READY");
        self.svc_ready = true;
        self.send_init(htc, ce);
    }

    fn on_ready(&mut self, body: &[u8]) {
        for (off, tag, len) in TlvIter::new(body) {
            if tag == 0x000C && len >= 6 {
                self.mac.copy_from_slice(&body[off..off + 6]);
            }
        }
        self.ready = true;
        crate::log!(
            "SWEET WLAN: wmi: READY (MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5]
        );
    }

    fn send_init(&self, htc: &mut HtcState, ce: &mut CeEngine) {
        let rc_bytes = 44 * 4;
        let total = WMI_CMD_HDR + TLV_HDR + 4 + TLV_HDR + rc_bytes;
        let mut m = [0u8; 512];
        put32(&mut m, 0, CMD_INIT);
        let mut o = WMI_CMD_HDR;
        o = tlv(&mut m, o, TAG_INIT_CMD, 4);
        put32(&mut m, o, 1);
        o += 4;
        o = tlv(&mut m, o, TAG_RESOURCE_CONFIG, rc_bytes);
        for (i, &v) in RES_CFG.iter().enumerate() {
            put32(&mut m, o + i * 4, v);
        }
        o += rc_bytes;
        let ok = htc.wmi_send(ce, &m[..o]);
        crate::log!("SWEET WLAN: wmi: INIT sent ({}), {} bytes", if ok { "ok" } else { "FAILED" }, o);
    }

    fn parse_mgmt_rx<'a>(&self, body: &'a [u8]) -> Option<WmiEvent<'a>> {
        let mut freq = 0u16;
        let mut rssi = 0i8;
        let mut frame_off = 0usize;
        let mut frame_len = 0usize;
        for (off, tag, len) in TlvIter::new(body) {
            if tag == TAG_MGMT_RX_HDR && len >= 20 {
                let ch = u32::from_le_bytes([body[off], body[off + 1], body[off + 2], body[off + 3]]);
                freq = ch as u16;
                let snr = u32::from_le_bytes([body[off + 8], body[off + 9], body[off + 10], body[off + 11]]);
                rssi = snr as i8;
                let buf_len = u32::from_le_bytes([body[off + 16], body[off + 17], body[off + 18], body[off + 19]]);
                frame_len = buf_len as usize;
            }
            if tag == TAG_ARRAY_BYTE {
                frame_off = off;
                if frame_len == 0 || frame_len > len {
                    frame_len = len;
                }
            }
        }
        if frame_off != 0 && frame_len > 0 && frame_off + frame_len <= body.len() {
            Some(WmiEvent::MgmtRx { freq, rssi, frame: &body[frame_off..frame_off + frame_len] })
        } else {
            None
        }
    }

    pub fn scan_start(&self, htc: &mut HtcState, ce: &mut CeEngine) -> bool {
        let nfreq = SCAN_FREQS.len();
        let ch_bytes = nfreq * 20;
        let bssid_bytes = 6;
        let ssid_bytes = 32;
        let ie_bytes = 0;
        let total = WMI_CMD_HDR
            + TLV_HDR + 56       // scan_cmd fixed
            + TLV_HDR + ch_bytes // channels
            + TLV_HDR + bssid_bytes
            + TLV_HDR + ssid_bytes
            + TLV_HDR + ie_bytes;
        let mut m = [0u8; 2048];
        put32(&mut m, 0, CMD_START_SCAN);
        let mut o = WMI_CMD_HDR;
        o = tlv(&mut m, o, TAG_SCAN_CMD_FIXED, 56);
        let fix = o;
        put32(&mut m, fix, 1);       // scan_id
        put32(&mut m, fix + 4, 1);   // scan_req_id
        put32(&mut m, fix + 8, 0);   // vdev_id
        put32(&mut m, fix + 12, 2);  // scan_priority
        put32(&mut m, fix + 16, 0x2000780C); // flags: passive
        put32(&mut m, fix + 20, 10000); // dwell_time_active
        put32(&mut m, fix, 1);
        put32(&mut m, fix + 24, 10000); // dwell_time_passive
        put32(&mut m, fix + 28, 300);   // min_rest_time
        put32(&mut m, fix + 32, 300);   // max_rest_time
        put32(&mut m, fix + 44, nfreq as u32); // num_chan
        o += 56;

        o = tlv(&mut m, o, TAG_SCAN_CHANNEL, ch_bytes);
        for (i, &f) in SCAN_FREQS.iter().enumerate() {
            let co = o + i * 20;
            put32(&mut m, co, f as u32);       // mhz
            put32(&mut m, co + 4, f as u32);   // cfreq1
            put32(&mut m, co + 8, 0);          // cfreq2
            let info = if f < 3000 { 1u32 } else { 6u32 };
            put32(&mut m, co + 12, info);      // info (flags+mode)
            put32(&mut m, co + 16, 0);         // reg_info
        }
        o += ch_bytes;

        o = tlv(&mut m, o, TAG_SCAN_BSSID, bssid_bytes);
        m[o..o + 6].copy_from_slice(&[0xFF; 6]);
        o += bssid_bytes;
        let pad = (4 - (bssid_bytes % 4)) % 4;
        o += pad;

        o = tlv(&mut m, o, TAG_SCAN_SSID, ssid_bytes);
        o += ssid_bytes;

        o = tlv(&mut m, o, TAG_SCAN_IE, ie_bytes);

        htc.wmi_send(ce, &m[..o])
    }

    pub fn cmd_raw(&self, htc: &mut HtcState, ce: &mut CeEngine, buf: &mut [u8], len: usize) -> bool {
        htc.wmi_send(ce, &buf[..len])
    }
}

pub fn scan_event_parse(body: &[u8]) -> Option<(u32, u32, u32)> {
    for (off, tag, len) in TlvIter::new(body) {
        if tag == TAG_SCAN_EV && len >= 12 {
            let ev = u32::from_le_bytes([body[off], body[off + 1], body[off + 2], body[off + 3]]);
            let reason = u32::from_le_bytes([body[off + 4], body[off + 5], body[off + 6], body[off + 7]]);
            let status = u32::from_le_bytes([body[off + 8], body[off + 9], body[off + 10], body[off + 11]]);
            return Some((ev, reason, status));
        }
    }
    None
}

fn put32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn put16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn tlv(buf: &mut [u8], off: usize, tag: u16, len: usize) -> usize {
    put16(buf, off, tag);
    put16(buf, off + 2, len as u16);
    off + TLV_HDR
}

pub fn put32_pub(buf: &mut [u8], off: usize, v: u32) {
    put32(buf, off, v);
}

pub fn put16_pub(buf: &mut [u8], off: usize, v: u16) {
    put16(buf, off, v);
}

pub fn tlv_pub(buf: &mut [u8], off: usize, tag: u16, len: usize) -> usize {
    tlv(buf, off, tag, len)
}

pub fn wmi_hdr(buf: &mut [u8], cmd: u32) {
    put32(buf, 0, cmd);
    buf[4] = 0;
    buf[5] = 0;
}

struct TlvIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TlvIter<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl<'a> Iterator for TlvIter<'a> {
    type Item = (usize, u16, usize);
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos + TLV_HDR > self.data.len() {
            return None;
        }
        let tag = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        let len = u16::from_le_bytes([self.data[self.pos + 2], self.data[self.pos + 3]]) as usize;
        let val_off = self.pos + TLV_HDR;
        let padded = (len + 3) & !3;
        self.pos = val_off + padded;
        if val_off + len > self.data.len() {
            return None;
        }
        Some((val_off, tag, len))
    }
}
