use core::cmp::min;

pub const MAX_BSS: usize = 64;
pub const MAX_IE: usize = 768;

#[repr(C)]
pub struct Bss {
    pub bssid: [u8; 6],
    pub ssid: [u8; 33],
    pub chan: u8,
    pub rssi: i8,
    pub seen: u16,
    pub raw_ssid: [u8; 32],
    pub raw_ssid_len: u8,
    pub freq: u16,
    pub bintval: u16,
    pub capab: u16,
    pub ie_len: u16,
    pub last_ms: u32,
    pub ie: [u8; MAX_IE],
}

impl Bss {
    pub fn new() -> Self {
        Self {
            bssid: [0; 6], ssid: [0; 33], chan: 0, rssi: -128, seen: 0,
            raw_ssid: [0; 32], raw_ssid_len: 0, freq: 0, bintval: 0, capab: 0,
            ie_len: 0, last_ms: 0, ie: [0; MAX_IE],
        }
    }

    pub fn best_rssi(table: &[Bss], n: usize, ssid: &[u8], bssid: Option<&[u8; 6]>) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut best_rssi = -128i8;
        for i in 0..n {
            let b = &table[i];
            if b.raw_ssid_len as usize != ssid.len() || b.raw_ssid[..ssid.len()] != *ssid {
                continue;
            }
            if let Some(bss) = bssid {
                if &b.bssid != bss {
                    continue;
                }
            }
            if b.rssi > best_rssi {
                best_rssi = b.rssi;
                best = Some(i);
            }
        }
        best
    }
}

#[repr(C)]
pub struct WlanBss {
    pub bssid: [u8; 6],
    pub ssid: [u8; 32],
    pub ssid_length: u8,
    pub frequency: u16,
    pub rssi: i16,
    pub beacon_interval: u16,
    pub capability: u16,
    pub age_ms: u32,
    pub elements_length: u16,
    pub elements: [u8; MAX_IE],
}

pub struct ScanState {
    pub bss: [Bss; MAX_BSS],
    pub nbss: u32,
    pub scans: u32,
    pub scan_frames: u32,
    pub scan_started: bool,
    pub scanning: bool,
    pub next_scan_ms: u32,
    pub scan_asked: bool,
    pub scans_done: u32,
}

impl ScanState {
    pub fn new() -> Self {
        Self {
            bss: core::array::from_fn(|_| Bss::new()),
            nbss: 0,
            scans: 0,
            scan_frames: 0,
            scan_started: false,
            scanning: false,
            next_scan_ms: 0,
            scan_asked: false,
            scans_done: 0,
        }
    }

    pub fn on_scan_event(&mut self, ev: u32, htt_msdus: u32) {
        if ev == 0x002 || ev == 0x040 || ev == 0x010 {
            self.scanning = false;
            self.scans += 1;
            crate::log!(
                "SWEET WLAN: scan {} done (event {:#x}): {} networks, {} beacons, {} htt msdus",
                self.scans, ev, self.nbss, self.scan_frames, htt_msdus
            );
            self.next_scan_ms = if self.scans < 3 { 10000 } else { 0 };
            self.scans_done += 1;
        }
    }

    pub fn mgmt_rx(&mut self, _freq: u16, rssi: i8, frame: &[u8], ms: u32) {
        if frame.len() < 36 {
            return;
        }
        let fc = u16::from_le_bytes([frame[0], frame[1]]);
        let subtype = (fc >> 4) & 0xF;
        if subtype != 8 && subtype != 5 {
            return;
        }
        self.scan_frames += 1;
        let flen = frame.len();

        let mut ssid = [0u8; 33];
        let mut ssid_len = 0usize;
        let mut chan = 0u8;
        let mut o = 36usize;
        while o + 2 <= flen && o + 2 + frame[o + 1] as usize <= flen {
            let id = frame[o];
            let elen = frame[o + 1] as usize;
            if id == 0 && elen <= 32 {
                ssid_len = elen;
                ssid[..elen].copy_from_slice(&frame[o + 2..o + 2 + elen]);
            }
            if id == 3 && elen >= 1 {
                chan = frame[o + 2];
            }
            o += 2 + elen;
        }

        let idx = self.find_or_add(&frame[16..22]);
        let idx = match idx {
            Some(i) => i,
            None => return,
        };
        let fresh = self.bss[idx].seen == 0;
        let b = &mut self.bss[idx];
        b.seen += 1;
        b.rssi = rssi;
        b.chan = chan;
        b.freq = if chan == 14 { 2484 } else if chan < 14 { 2407 + 5 * chan as u16 } else { 5000 + 5 * chan as u16 };
        b.last_ms = ms;
        b.bintval = u16::from_le_bytes([frame[32], frame[33]]);
        b.capab = u16::from_le_bytes([frame[34], frame[35]]);
        let ie_avail = min(flen - 36, MAX_IE);
        b.ie_len = ie_avail as u16;
        b.ie[..ie_avail].copy_from_slice(&frame[36..36 + ie_avail]);
        b.ssid[..ssid_len].copy_from_slice(&ssid[..ssid_len]);
        if ssid_len < 33 {
            b.ssid[ssid_len] = 0;
        }
        b.raw_ssid_len = 0;
        o = 36;
        while o + 2 <= flen && o + 2 + frame[o + 1] as usize <= flen {
            if frame[o] == 0 {
                let elen = min(frame[o + 1] as usize, 32);
                b.raw_ssid_len = elen as u8;
                b.raw_ssid[..elen].copy_from_slice(&frame[o + 2..o + 2 + elen]);
                break;
            }
            o += 2 + frame[o + 1] as usize;
        }

        if fresh {
            crate::log!(
                "SWEET WLAN: net {:2}: {:02x}:{:02x}:{:02x}:**:**:** ch {:3} {:4} dBm",
                self.nbss, b.bssid[0], b.bssid[1], b.bssid[2], b.chan, b.rssi
            );
        }
    }

    fn find_or_add(&mut self, bssid: &[u8]) -> Option<usize> {
        for i in 0..self.nbss as usize {
            if self.bss[i].bssid == bssid[..6] {
                return Some(i);
            }
        }
        if self.nbss as usize >= MAX_BSS {
            return None;
        }
        let idx = self.nbss as usize;
        self.bss[idx] = Bss::new();
        self.bss[idx].bssid.copy_from_slice(&bssid[..6]);
        self.nbss += 1;
        Some(idx)
    }

    pub fn fill_wlan_bss(&self, idx: usize, ms: u32) -> Option<WlanBss> {
        if idx >= self.nbss as usize {
            return None;
        }
        let b = &self.bss[idx];
        let mut out = WlanBss {
            bssid: b.bssid,
            ssid: [0; 32],
            ssid_length: b.raw_ssid_len,
            frequency: b.freq,
            rssi: b.rssi as i16,
            beacon_interval: b.bintval,
            capability: b.capab,
            age_ms: ms.wrapping_sub(b.last_ms),
            elements_length: min(b.ie_len, MAX_IE as u16),
            elements: [0; MAX_IE],
        };
        let sl = b.raw_ssid_len as usize;
        out.ssid[..sl].copy_from_slice(&b.raw_ssid[..sl]);
        let el = out.elements_length as usize;
        out.elements[..el].copy_from_slice(&b.ie[..el]);
        Some(out)
    }
}
