use super::ce::CeEngine;
use super::htc::HtcState;
use super::htt::HttState;
use super::scan::{Bss, ScanState};
use super::wmi;
use super::crypto;

const LLC_SNAP: [u8; 6] = [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00];
const EAPOL_ETYPE: [u8; 2] = [0x88, 0x8E];

const TX_QUEUE_SIZE: usize = 32;

#[derive(Clone, Copy, PartialEq)]
pub enum StaPhase {
    Idle,
    Start,
    Auth,
    Assoc,
    FourWay,
    Up,
}

pub struct StaState {
    pub phase: StaPhase,
    pub ssid: [u8; 32],
    pub ssid_len: u8,
    pub bssid: [u8; 6],
    pub pmk: [u8; 32],
    pub ptk: [u8; 48],
    pub gtk: [u8; 32],
    pub gtk_idx: u8,
    anonce: [u8; 32],
    snonce: [u8; 32],
    pub freq: u16,
    pub bintval: u16,
    pub dtim: u8,
    pub join_asked: bool,
    pub join_done: u32,
    pub join_result: i32,
    pub leave_asked: bool,
    pub vdev_start_ok: bool,
    tx_queue: [[u8; 2048]; TX_QUEUE_SIZE],
    tx_len: [usize; TX_QUEUE_SIZE],
    tx_head: usize,
    tx_tail: usize,
    tx_count: usize,
    pub rx_hook: Option<extern "C" fn(*const u8, usize)>,
    pub link_hook: Option<extern "C" fn(bool)>,
}

impl StaState {
    pub fn new() -> Self {
        Self {
            phase: StaPhase::Idle,
            ssid: [0; 32], ssid_len: 0,
            bssid: [0; 6], pmk: [0; 32], ptk: [0; 48], gtk: [0; 32], gtk_idx: 0,
            anonce: [0; 32], snonce: [0; 32],
            freq: 0, bintval: 100, dtim: 1,
            join_asked: false, join_done: 0, join_result: 0, leave_asked: false,
            vdev_start_ok: false,
            tx_queue: [[0u8; 2048]; TX_QUEUE_SIZE],
            tx_len: [0; TX_QUEUE_SIZE],
            tx_head: 0, tx_tail: 0, tx_count: 0,
            rx_hook: None, link_hook: None,
        }
    }

    pub fn on_vdev_start_resp(&mut self, status: u32) {
        self.vdev_start_ok = status == 0;
        crate::log!("SWEET WLAN: sta: VDEV_START_RESP status={}", status);
    }

    pub fn drive(
        &mut self,
        htc: &mut HtcState,
        ce: &mut CeEngine,
        _htt: &mut HttState,
        scan: &ScanState,
        our_mac: &[u8; 6],
    ) {
        if self.leave_asked && self.phase != StaPhase::Idle {
            self.do_leave(htc, ce);
            return;
        }
        if !self.join_asked {
            return;
        }
        match self.phase {
            StaPhase::Idle => {
                let sl = self.ssid_len as usize;
                let bssid_filter = if self.bssid != [0; 6] { Some(&self.bssid) } else { None };
                let idx = Bss::best_rssi(&scan.bss, scan.nbss as usize, &self.ssid[..sl], bssid_filter);
                let idx = match idx {
                    Some(i) => i,
                    None => {
                        self.finish_join(2); // ENOENT
                        return;
                    }
                };
                let b = &scan.bss[idx];
                self.bssid.copy_from_slice(&b.bssid);
                self.freq = b.freq;
                self.bintval = b.bintval;
                self.dtim = 1;
                for o_ie in IeIter::new(&b.ie[..b.ie_len as usize]) {
                    if o_ie.0 == 5 && o_ie.1.len() >= 2 {
                        self.dtim = o_ie.1[1];
                    }
                }
                crate::log!(
                    "SWEET WLAN: sta: joining {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} freq={} bintval={}",
                    self.bssid[0], self.bssid[1], self.bssid[2],
                    self.bssid[3], self.bssid[4], self.bssid[5],
                    self.freq, self.bintval
                );
                self.vdev_start(htc, ce, our_mac);
                self.phase = StaPhase::Start;
            }
            StaPhase::Start => {
                if !self.vdev_start_ok {
                    return;
                }
                self.peer_create(htc, ce);
                self.send_auth(htc, ce, our_mac);
                self.phase = StaPhase::Auth;
            }
            _ => {}
        }
    }

    pub fn on_mgmt_rx(
        &mut self,
        htc: &mut HtcState,
        ce: &mut CeEngine,
        frame: &[u8],
        our_mac: &[u8; 6],
    ) {
        if frame.len() < 24 {
            return;
        }
        let fc = u16::from_le_bytes([frame[0], frame[1]]);
        let subtype = (fc >> 4) & 0xF;
        match (self.phase, subtype) {
            (StaPhase::Auth, 11) => {
                let status = u16::from_le_bytes([frame[28], frame[29]]);
                crate::log!("SWEET WLAN: sta: AUTH response status={}", status);
                if status != 0 {
                    self.finish_join(45); // EAUTH
                    return;
                }
                self.send_assoc_req(htc, ce, our_mac);
                self.phase = StaPhase::Assoc;
            }
            (StaPhase::Assoc, 1) => {
                if frame.len() < 30 {
                    return;
                }
                let status = u16::from_le_bytes([frame[26], frame[27]]);
                crate::log!("SWEET WLAN: sta: ASSOC response status={}", status);
                if status != 0 {
                    self.finish_join(45);
                    return;
                }
                self.peer_assoc(htc, ce);
                self.vdev_up(htc, ce);
                if self.pmk == [0; 32] {
                    self.phase = StaPhase::Up;
                    self.finish_join(0);
                    if let Some(h) = self.link_hook { h(true); }
                } else {
                    self.phase = StaPhase::FourWay;
                }
            }
            _ => {}
        }
    }

    pub fn on_data_rx(
        &mut self,
        htc: &mut HtcState,
        ce: &mut CeEngine,
        frame: &[u8],
        our_mac: &[u8; 6],
    ) {
        if self.phase != StaPhase::FourWay && self.phase != StaPhase::Up {
            return;
        }
        let (eth, eth_len) = match native_wifi_to_eth(frame, our_mac) {
            Some(e) => e,
            None => return,
        };
        if eth_len >= 14 && eth[12] == EAPOL_ETYPE[0] && eth[13] == EAPOL_ETYPE[1] {
            self.on_eapol(htc, ce, &eth[14..eth_len], our_mac);
        } else if self.phase == StaPhase::Up {
            if let Some(h) = self.rx_hook {
                h(eth.as_ptr(), eth_len);
            }
        }
    }

    fn on_eapol(
        &mut self,
        htc: &mut HtcState,
        ce: &mut CeEngine,
        eap: &[u8],
        our_mac: &[u8; 6],
    ) {
        if eap.len() < 99 {
            return;
        }
        let key_info = u16::from_be_bytes([eap[5], eap[6]]);
        let _key_len = u16::from_be_bytes([eap[7], eap[8]]) as usize;
        let has_mic = key_info & 0x0100 != 0;
        let is_install = key_info & 0x0040 != 0;
        let is_ack = key_info & 0x0080 != 0;

        if is_ack && !has_mic {
            crate::log!("SWEET WLAN: sta: EAPOL msg 1 (ANonce)");
            self.anonce.copy_from_slice(&eap[17..49]);
            fill_random(&mut self.snonce);
            self.derive_ptk(our_mac);
            self.send_eapol_msg2(htc, ce, our_mac);
        } else if is_ack && has_mic && is_install {
            crate::log!("SWEET WLAN: sta: EAPOL msg 3");
            let mic_ok = self.verify_mic(eap);
            if !mic_ok {
                crate::log!("SWEET WLAN: sta: msg3 MIC FAILED");
                self.finish_join(45);
                return;
            }
            let key_data_len = u16::from_be_bytes([eap[97], eap[98]]) as usize;
            if eap.len() >= 99 + key_data_len && key_data_len >= 24 {
                self.unwrap_gtk(&eap[99..99 + key_data_len]);
            }
            self.send_eapol_msg4(htc, ce, our_mac);
            self.install_keys(htc, ce, our_mac);
            self.phase = StaPhase::Up;
            self.finish_join(0);
            crate::log!("SWEET WLAN: sta: 4-way COMPLETE, link UP");
            if let Some(h) = self.link_hook { h(true); }
        }
    }

    fn derive_ptk(&mut self, our_mac: &[u8; 6]) {
        let mut data = [0u8; 76];
        let (aa, spa) = (&self.bssid, our_mac);
        if spa < aa {
            data[..6].copy_from_slice(spa);
            data[6..12].copy_from_slice(aa);
        } else {
            data[..6].copy_from_slice(aa);
            data[6..12].copy_from_slice(spa);
        }
        if self.snonce < self.anonce {
            data[12..44].copy_from_slice(&self.snonce);
            data[44..76].copy_from_slice(&self.anonce);
        } else {
            data[12..44].copy_from_slice(&self.anonce);
            data[44..76].copy_from_slice(&self.snonce);
        }
        crypto::prf(&self.pmk, b"Pairwise key expansion", &data, &mut self.ptk);
    }

    fn verify_mic(&self, eap: &[u8]) -> bool {
        let mut tmp = [0u8; 2048];
        let len = core::cmp::min(eap.len(), tmp.len());
        tmp[..len].copy_from_slice(&eap[..len]);
        for i in 81..97 {
            tmp[i] = 0;
        }
        let mic = crypto::hmac_sha1(&self.ptk[..16], &[&tmp[..len]]);
        mic[..16] == eap[81..97]
    }

    fn unwrap_gtk(&mut self, kd: &[u8]) {
        let kek = &self.ptk[16..32];
        let mut plain = [0u8; 256];
        let mut kek_arr = [0u8; 16];
        kek_arr.copy_from_slice(kek);
        if crypto::aes_unwrap(&kek_arr, kd, &mut plain) {
            let mut off = 0usize;
            while off + 2 <= kd.len() - 8 {
                let id = plain[off];
                let elen = plain[off + 1] as usize;
                if id == 0xDD && elen >= 8 {
                    self.gtk_idx = plain[off + 4] & 3;
                    let gk_len = core::cmp::min(elen - 6, 32);
                    self.gtk[..gk_len].copy_from_slice(&plain[off + 8..off + 8 + gk_len]);
                    break;
                }
                off += 2 + elen;
            }
        }
    }

    fn send_eapol_msg2(&self, htc: &mut HtcState, ce: &mut CeEngine, our_mac: &[u8; 6]) {
        let rsn = self.build_rsn_ie();
        let rsn_len = rsn.1;
        let eap_len = 99 + rsn_len;
        let mut eap = [0u8; 256];
        eap[0] = 0x02; // version
        eap[1] = 0x03; // key type
        let body_len = (eap_len - 4) as u16;
        eap[2..4].copy_from_slice(&body_len.to_be_bytes());
        eap[4] = 0x02; // descriptor RSN
        let ki: u16 = 0x010A; // pairwise + HMAC-SHA1-MIC
        eap[5..7].copy_from_slice(&ki.to_be_bytes());
        eap[9..11].copy_from_slice(&0u16.to_be_bytes()); // key length = 0 for msg2
        eap[17..49].copy_from_slice(&self.snonce);
        eap[97..99].copy_from_slice(&(rsn_len as u16).to_be_bytes());
        eap[99..99 + rsn_len].copy_from_slice(&rsn.0[..rsn_len]);
        let mic = crypto::hmac_sha1(&self.ptk[..16], &[&eap[..eap_len]]);
        eap[81..97].copy_from_slice(&mic[..16]);
        self.tx_eapol(htc, ce, &eap[..eap_len], our_mac);
    }

    fn send_eapol_msg4(&self, htc: &mut HtcState, ce: &mut CeEngine, our_mac: &[u8; 6]) {
        let eap_len = 99;
        let mut eap = [0u8; 128];
        eap[0] = 0x02;
        eap[1] = 0x03;
        let body_len = (eap_len - 4) as u16;
        eap[2..4].copy_from_slice(&body_len.to_be_bytes());
        eap[4] = 0x02;
        let ki: u16 = 0x030A;
        eap[5..7].copy_from_slice(&ki.to_be_bytes());
        let mic = crypto::hmac_sha1(&self.ptk[..16], &[&eap[..eap_len]]);
        eap[81..97].copy_from_slice(&mic[..16]);
        self.tx_eapol(htc, ce, &eap[..eap_len], our_mac);
    }

    fn tx_eapol(&self, htc: &mut HtcState, ce: &mut CeEngine, eap: &[u8], our_mac: &[u8; 6]) {
        let frame = eth_to_native_wifi(our_mac, &self.bssid, &EAPOL_ETYPE, eap);
        if let Some((buf, len)) = frame {
            self.mgmt_tx(htc, ce, &buf[..len]);
        }
    }

    fn install_keys(&self, htc: &mut HtcState, ce: &mut CeEngine, our_mac: &[u8; 6]) {
        self.install_key(htc, ce, 0, 0, &self.ptk[32..48], our_mac);
        self.install_key(htc, ce, self.gtk_idx, 1, &self.gtk[..16], &[0xFF; 6]);
        self.peer_authorize(htc, ce);
    }

    fn install_key(
        &self,
        htc: &mut HtcState,
        ce: &mut CeEngine,
        idx: u8,
        is_group: u8,
        key: &[u8],
        peer: &[u8],
    ) {
        let mut m = [0u8; 512];
        wmi::wmi_hdr(&mut m, wmi::CMD_VDEV_INSTALL_KEY);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x0053, 80);
        wmi::put32_pub(&mut m, o, 0);       // vdev_id
        m[o + 4..o + 10].copy_from_slice(&peer[..6]);
        wmi::put32_pub(&mut m, o + 12, idx as u32); // key_idx
        wmi::put32_pub(&mut m, o + 16, 0x04); // CCMP cipher
        wmi::put32_pub(&mut m, o + 20, key.len() as u32);
        wmi::put32_pub(&mut m, o + 28, is_group as u32); // key_flags: 0=pairwise, 1=group
        m[o + 32..o + 32 + key.len()].copy_from_slice(key);
        o += 80;
        htc.wmi_send(ce, &m[..o]);
    }

    fn peer_authorize(&self, htc: &mut HtcState, ce: &mut CeEngine) {
        let mut m = [0u8; 128];
        wmi::wmi_hdr(&mut m, wmi::CMD_PEER_SET_PARAM);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x004E, 20);
        wmi::put32_pub(&mut m, o, 0);    // vdev_id
        m[o + 4..o + 10].copy_from_slice(&self.bssid);
        wmi::put32_pub(&mut m, o + 12, 1); // param_id: authorize
        wmi::put32_pub(&mut m, o + 16, 1); // value
        o += 20;
        htc.wmi_send(ce, &m[..o]);
    }

    fn vdev_start(&self, htc: &mut HtcState, ce: &mut CeEngine, _our_mac: &[u8; 6]) {
        let mut m = [0u8; 256];
        wmi::wmi_hdr(&mut m, wmi::CMD_VDEV_START);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x0045, 80);
        wmi::put32_pub(&mut m, o, 0);    // vdev_id
        wmi::put32_pub(&mut m, o + 12, self.freq as u32);
        wmi::put32_pub(&mut m, o + 16, self.freq as u32);
        wmi::put32_pub(&mut m, o + 36, self.bintval as u32);
        wmi::put32_pub(&mut m, o + 40, self.dtim as u32);
        o += 80;
        o = wmi::tlv_pub(&mut m, o, 0x0025, 20);
        wmi::put32_pub(&mut m, o, self.freq as u32);
        wmi::put32_pub(&mut m, o + 4, self.freq as u32);
        let mode = if self.freq < 3000 { 1u32 } else { 6u32 };
        wmi::put32_pub(&mut m, o + 12, mode);
        o += 20;
        htc.wmi_send(ce, &m[..o]);
    }

    fn vdev_up(&self, htc: &mut HtcState, ce: &mut CeEngine) {
        let mut m = [0u8; 64];
        wmi::wmi_hdr(&mut m, wmi::CMD_VDEV_UP);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x0046, 16);
        wmi::put32_pub(&mut m, o, 0);    // vdev_id
        wmi::put32_pub(&mut m, o + 4, 0); // assoc_id
        m[o + 8..o + 14].copy_from_slice(&self.bssid);
        o += 16;
        htc.wmi_send(ce, &m[..o]);
    }

    fn peer_create(&self, htc: &mut HtcState, ce: &mut CeEngine) {
        let mut m = [0u8; 64];
        wmi::wmi_hdr(&mut m, wmi::CMD_PEER_CREATE);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x004D, 12);
        wmi::put32_pub(&mut m, o, 0); // vdev_id
        m[o + 4..o + 10].copy_from_slice(&self.bssid);
        o += 12;
        htc.wmi_send(ce, &m[..o]);
    }

    fn peer_assoc(&self, htc: &mut HtcState, ce: &mut CeEngine) {
        let mut m = [0u8; 256];
        wmi::wmi_hdr(&mut m, wmi::CMD_PEER_ASSOC);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x004F, 60);
        wmi::put32_pub(&mut m, o, 0); // vdev_id
        m[o + 4..o + 10].copy_from_slice(&self.bssid);
        wmi::put32_pub(&mut m, o + 12, 1); // listen_interval
        wmi::put32_pub(&mut m, o + 16, 0); // assoc_id
        o += 60;
        htc.wmi_send(ce, &m[..o]);
    }

    fn send_auth(&self, htc: &mut HtcState, ce: &mut CeEngine, our_mac: &[u8; 6]) {
        let mut f = [0u8; 30];
        f[0] = 0xB0; // auth frame
        f[4..10].copy_from_slice(&self.bssid); // DA
        f[10..16].copy_from_slice(our_mac);    // SA
        f[16..22].copy_from_slice(&self.bssid); // BSSID
        // auth algo=0 (open), seq=1, status=0 -> already zeroed
        f[24..26].copy_from_slice(&0u16.to_le_bytes()); // algo
        f[26..28].copy_from_slice(&1u16.to_le_bytes()); // seq
        f[28..30].copy_from_slice(&0u16.to_le_bytes()); // status
        self.mgmt_tx(htc, ce, &f);
    }

    fn send_assoc_req(&self, htc: &mut HtcState, ce: &mut CeEngine, our_mac: &[u8; 6]) {
        let sl = self.ssid_len as usize;
        let rsn = self.build_rsn_ie();
        let rsn_len = rsn.1;
        let total = 24 + 4 + 2 + sl + rsn_len;
        let mut f = [0u8; 512];
        f[0] = 0x00; // assoc req
        f[4..10].copy_from_slice(&self.bssid);
        f[10..16].copy_from_slice(our_mac);
        f[16..22].copy_from_slice(&self.bssid);
        // capabilities + listen interval
        f[24..26].copy_from_slice(&0x0431u16.to_le_bytes()); // ESS + privacy + short slot
        f[26..28].copy_from_slice(&10u16.to_le_bytes());
        let mut o = 28;
        f[o] = 0; // SSID IE
        f[o + 1] = sl as u8;
        f[o + 2..o + 2 + sl].copy_from_slice(&self.ssid[..sl]);
        o += 2 + sl;
        if rsn_len > 0 {
            f[o..o + rsn_len].copy_from_slice(&rsn.0[..rsn_len]);
            o += rsn_len;
        }
        self.mgmt_tx(htc, ce, &f[..o]);
    }

    fn mgmt_tx(&self, htc: &mut HtcState, ce: &mut CeEngine, frame: &[u8]) {
        let mut m = [0u8; 1024];
        wmi::wmi_hdr(&mut m, wmi::CMD_MGMT_TX_SEND);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x005B, 16);
        wmi::put32_pub(&mut m, o, 0);    // vdev_id
        wmi::put32_pub(&mut m, o + 4, 0); // desc_id
        wmi::put32_pub(&mut m, o + 8, self.freq as u32);
        wmi::put32_pub(&mut m, o + 12, frame.len() as u32);
        o += 16;
        o = wmi::tlv_pub(&mut m, o, 0x0101, frame.len());
        m[o..o + frame.len()].copy_from_slice(frame);
        o += frame.len();
        let pad = (4 - (frame.len() % 4)) % 4;
        o += pad;
        htc.wmi_send(ce, &m[..o]);
    }

    fn build_rsn_ie(&self) -> ([u8; 64], usize) {
        let mut ie = [0u8; 64];
        if self.pmk == [0; 32] {
            return (ie, 0);
        }
        ie[0] = 0x30; // RSN IE id
        ie[1] = 20;   // length
        ie[2] = 1; ie[3] = 0; // version
        ie[4..8].copy_from_slice(&[0x00, 0x0F, 0xAC, 0x04]); // group CCMP
        ie[8..10].copy_from_slice(&1u16.to_le_bytes()); // pairwise count
        ie[10..14].copy_from_slice(&[0x00, 0x0F, 0xAC, 0x04]); // pairwise CCMP
        ie[14..16].copy_from_slice(&1u16.to_le_bytes()); // akm count
        ie[16..20].copy_from_slice(&[0x00, 0x0F, 0xAC, 0x02]); // PSK
        ie[20..22].copy_from_slice(&0u16.to_le_bytes()); // capabilities
        (ie, 22)
    }

    fn do_leave(&mut self, htc: &mut HtcState, ce: &mut CeEngine) {
        crate::log!("SWEET WLAN: sta: leaving");
        if self.phase == StaPhase::Up {
            if let Some(h) = self.link_hook { h(false); }
        }
        let mut m = [0u8; 64];
        wmi::wmi_hdr(&mut m, wmi::CMD_VDEV_DOWN);
        htc.wmi_send(ce, &m[..6]);
        wmi::wmi_hdr(&mut m, wmi::CMD_PEER_DELETE);
        let mut o = 6;
        o = wmi::tlv_pub(&mut m, o, 0x004D, 12);
        wmi::put32_pub(&mut m, o, 0);
        m[o + 4..o + 10].copy_from_slice(&self.bssid);
        o += 12;
        htc.wmi_send(ce, &m[..o]);
        wmi::wmi_hdr(&mut m, wmi::CMD_VDEV_STOP);
        htc.wmi_send(ce, &m[..6]);
        self.phase = StaPhase::Idle;
        self.leave_asked = false;
        self.join_asked = false;
        self.vdev_start_ok = false;
        self.tx_head = 0;
        self.tx_tail = 0;
        self.tx_count = 0;
    }

    fn finish_join(&mut self, err: i32) {
        self.join_result = err;
        self.join_asked = false;
        self.join_done += 1;
        if err != 0 {
            crate::log!("SWEET WLAN: sta: join failed (err={})", err);
        }
    }

    pub fn enqueue_tx(&mut self, frame: &[u8]) -> bool {
        if self.phase != StaPhase::Up || self.tx_count >= TX_QUEUE_SIZE || frame.len() > 2048 {
            return false;
        }
        self.tx_queue[self.tx_tail][..frame.len()].copy_from_slice(frame);
        self.tx_len[self.tx_tail] = frame.len();
        self.tx_tail = (self.tx_tail + 1) % TX_QUEUE_SIZE;
        self.tx_count += 1;
        true
    }

    pub fn pump_tx(
        &mut self,
        htc: &mut HtcState,
        ce: &mut CeEngine,
        htt: &mut HttState,
        our_mac: &[u8; 6],
    ) {
        while self.tx_count > 0 {
            let idx = self.tx_head;
            let len = self.tx_len[idx];
            let eth = &self.tx_queue[idx][..len];
            if eth.len() < 14 {
                self.tx_head = (self.tx_head + 1) % TX_QUEUE_SIZE;
                self.tx_count -= 1;
                continue;
            }
            let nw = match eth_to_native_wifi(our_mac, &self.bssid, &eth[12..14], &eth[14..]) {
                Some(f) => f,
                None => {
                    self.tx_head = (self.tx_head + 1) % TX_QUEUE_SIZE;
                    self.tx_count -= 1;
                    continue;
                }
            };
            if !htt.tx_frame(htc, ce, &nw.0[..nw.1]) {
                break;
            }
            self.tx_head = (self.tx_head + 1) % TX_QUEUE_SIZE;
            self.tx_count -= 1;
        }
    }
}

fn fill_random(buf: &mut [u8; 32]) {
    let mut s: u32 = 0x1337_CAFE;
    for b in buf.iter_mut() {
        s = s.wrapping_mul(1103515245).wrapping_add(12345);
        *b = (s >> 16) as u8;
    }
}

fn eth_to_native_wifi(sa: &[u8; 6], da: &[u8; 6], etype: &[u8], payload: &[u8]) -> Option<([u8; 2048], usize)> {
    let total = 24 + 2 + LLC_SNAP.len() + 2 + payload.len();
    if total > 2048 {
        return None;
    }
    let mut f = [0u8; 2048];
    f[0] = 0x08; f[1] = 0x01; // data, to-DS
    f[4..10].copy_from_slice(da);       // BSSID (RA)
    f[10..16].copy_from_slice(sa);      // SA
    f[16..22].copy_from_slice(da);      // DA
    let mut o = 24;
    f[o] = 0; f[o + 1] = 0; // QoS (not used but space)
    o += 2;
    f[o..o + 6].copy_from_slice(&LLC_SNAP);
    o += 6;
    f[o..o + 2].copy_from_slice(etype);
    o += 2;
    f[o..o + payload.len()].copy_from_slice(payload);
    o += payload.len();
    Some((f, o))
}

fn native_wifi_to_eth(frame: &[u8], _our_mac: &[u8; 6]) -> Option<([u8; 2048], usize)> {
    if frame.len() < 26 + LLC_SNAP.len() + 2 {
        return None;
    }
    let fc = u16::from_le_bytes([frame[0], frame[1]]);
    if fc & 0x000C != 0x0008 {
        return None;
    }
    let hdr_len = if fc & 0x0080 != 0 { 26 } else { 24 };
    let qos_off = if fc & 0x0080 != 0 { 2 } else { 0 };
    let llc_off = hdr_len + qos_off;
    if frame.len() < llc_off + 8 {
        return None;
    }
    let to_ds = fc & 0x0100 != 0;
    let from_ds = fc & 0x0200 != 0;
    let (da, sa) = match (to_ds, from_ds) {
        (false, true) => (&frame[4..10], &frame[16..22]),
        (true, false) => (&frame[16..22], &frame[10..16]),
        _ => return None,
    };
    let mut eth = [0u8; 2048];
    eth[0..6].copy_from_slice(da);
    eth[6..12].copy_from_slice(sa);
    eth[12..14].copy_from_slice(&frame[llc_off + 6..llc_off + 8]);
    let payload_off = llc_off + 8;
    let plen = frame.len() - payload_off;
    let total = 14 + plen;
    if total > 2048 {
        return None;
    }
    eth[14..14 + plen].copy_from_slice(&frame[payload_off..]);
    Some((eth, total))
}

struct IeIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> IeIter<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl<'a> Iterator for IeIter<'a> {
    type Item = (u8, &'a [u8]);
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos + 2 > self.data.len() {
            return None;
        }
        let id = self.data[self.pos];
        let len = self.data[self.pos + 1] as usize;
        if self.pos + 2 + len > self.data.len() {
            return None;
        }
        let val = &self.data[self.pos + 2..self.pos + 2 + len];
        self.pos += 2 + len;
        Some((id, val))
    }
}
