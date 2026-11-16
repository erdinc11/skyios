use super::ce::CeEngine;

const HTC_HDR_LEN: usize = 8;
const CE_HTC_CMD: usize = 0;
const CE_HTC_RX1: usize = 1;
const CE_HTC_RX2: usize = 2;

const MSG_READY: u16 = 0x0001;
const MSG_CONNECT_SVC: u16 = 0x0002;
const MSG_CONNECT_SVC_RESP: u16 = 0x0003;
const MSG_SETUP_COMPLETE_EX: u16 = 0x0004;

const SVC_WMI_CONTROL: u16 = 0x0100;
const SVC_HTT_DATA: u16 = 0x0300;

const WMI_QUEUE_SIZE: usize = 6;
const WMI_MAX_PAYLOAD: usize = 2040;

pub struct HtcState {
    pub wmi_eid: u8,
    pub htt_eid: u8,
    next_eid: u8,
    wmi_credits: u32,
    phase: u8,
    pub ready: bool,
}

impl HtcState {
    pub fn new() -> Self {
        Self {
            wmi_eid: 0xFF,
            htt_eid: 0xFF,
            next_eid: 2,
            wmi_credits: WMI_QUEUE_SIZE as u32,
            phase: 0,
            ready: false,
        }
    }

    pub fn drive(&mut self, ce: &mut CeEngine) {
        match self.phase {
            0 => {}
            1 => {
                crate::log!("SWEET WLAN: htc: connecting HTT_DATA (svc 0x0300)");
                self.connect(ce, SVC_HTT_DATA);
                self.phase = 2;
            }
            2 => {}
            3 => {
                crate::log!("SWEET WLAN: htc: connecting WMI_CONTROL (svc 0x0100)");
                self.connect(ce, SVC_WMI_CONTROL);
                self.phase = 4;
            }
            4 => {}
            5 => {
                self.setup_complete_ex(ce);
                self.phase = 6;
                self.ready = true;
                crate::log!("SWEET WLAN: htc: SETUP_COMPLETE_EX sent, radio ready");
            }
            _ => {}
        }
    }

    pub fn rx(&mut self, ce: &mut CeEngine, _c: usize, raw: &[u8]) -> Option<(u8, usize, usize)> {
        if raw.len() < HTC_HDR_LEN {
            return None;
        }
        let eid = raw[0];
        let _flags = raw[1];
        let len = u16::from_le_bytes([raw[2], raw[3]]) as usize;
        let _trailer = raw[4];
        let _seq = raw[5];
        let plen = core::cmp::min(len, raw.len() - HTC_HDR_LEN);

        if eid == 0 {
            self.handle_ep0(ce, &raw[HTC_HDR_LEN..HTC_HDR_LEN + plen]);
            return None;
        }
        Some((eid, HTC_HDR_LEN, plen))
    }

    fn handle_ep0(&mut self, ce: &mut CeEngine, p: &[u8]) {
        if p.len() < 2 {
            return;
        }
        let mid = u16::from_le_bytes([p[0], p[1]]);
        match mid {
            MSG_READY => {
                crate::log!("SWEET WLAN: htc: READY received");
                self.phase = 1;
                self.drive(ce);
            }
            MSG_CONNECT_SVC_RESP => {
                if p.len() < 10 {
                    return;
                }
                let svc = u16::from_le_bytes([p[2], p[3]]);
                let status = p[4];
                let eid = p[5];
                let max_msg = u16::from_le_bytes([p[6], p[7]]);
                crate::log!(
                    "SWEET WLAN: htc: CONNECT_SVC_RESP svc={:#06x} eid={} status={} max={}",
                    svc, eid, status, max_msg
                );
                if status != 0 {
                    return;
                }
                match svc {
                    SVC_HTT_DATA => {
                        self.htt_eid = eid;
                        self.phase = 3;
                        self.drive(ce);
                    }
                    SVC_WMI_CONTROL => {
                        self.wmi_eid = eid;
                        self.phase = 5;
                        self.drive(ce);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn connect(&self, ce: &mut CeEngine, svc: u16) {
        let mut m = [0u8; HTC_HDR_LEN + 12];
        m[2] = 12;
        let body = &mut m[HTC_HDR_LEN..];
        body[0..2].copy_from_slice(&MSG_CONNECT_SVC.to_le_bytes());
        body[2..4].copy_from_slice(&svc.to_le_bytes());
        body[4] = 0x02;
        ce.send(CE_HTC_CMD, &m, 0);
    }

    fn setup_complete_ex(&self, ce: &mut CeEngine) {
        let mut m = [0u8; HTC_HDR_LEN + 4];
        m[2] = 4;
        let body = &mut m[HTC_HDR_LEN..];
        body[0..2].copy_from_slice(&MSG_SETUP_COMPLETE_EX.to_le_bytes());
        ce.send(CE_HTC_CMD, &m, 0);
    }

    pub fn send(&mut self, ce: &mut CeEngine, eid: u8, data: &[u8]) -> bool {
        if data.len() > 2032 {
            return false;
        }
        let total = HTC_HDR_LEN + data.len();
        let mut m = [0u8; 2048];
        m[0] = eid;
        m[2..4].copy_from_slice(&(data.len() as u16).to_le_bytes());
        m[HTC_HDR_LEN..HTC_HDR_LEN + data.len()].copy_from_slice(data);
        ce.send(CE_HTC_CMD, &m[..total], 0)
    }

    pub fn wmi_send(&mut self, ce: &mut CeEngine, data: &[u8]) -> bool {
        if self.wmi_eid == 0xFF || data.len() > WMI_MAX_PAYLOAD {
            return false;
        }
        self.send(ce, self.wmi_eid, data)
    }

    pub fn htt_send(&mut self, ce: &mut CeEngine, data: &[u8]) -> bool {
        if self.htt_eid == 0xFF {
            return false;
        }
        let total = HTC_HDR_LEN + data.len();
        if total > 256 {
            let mut m = [0u8; 2048];
            m[0] = self.htt_eid;
            m[2..4].copy_from_slice(&(data.len() as u16).to_le_bytes());
            m[HTC_HDR_LEN..HTC_HDR_LEN + data.len()].copy_from_slice(data);
            ce.send(4, &m[..total], 0)
        } else {
            self.send(ce, self.htt_eid, data)
        }
    }

    pub fn htt_send_buf(&mut self, ce: &mut CeEngine, buf: &[u8]) -> bool {
        if self.htt_eid == 0xFF {
            return false;
        }
        ce.send(4, buf, 0)
    }
}
