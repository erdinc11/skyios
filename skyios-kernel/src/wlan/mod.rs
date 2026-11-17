pub mod ce;
pub mod crypto;
pub mod htc;
pub mod htt;
pub mod scan;
pub mod sta;
pub mod wmi;

use crate::xnu::{self, Mmio};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::ptr::addr_of_mut;

const WLAN_SMMU_PA: u64 = 0x0C60_0000;
const WLAN_SMMU_SZ: usize = 0x80000;
const WLAN_SID: u32 = 0x1A0;
const CB_CLONE_OFF: usize = 0x1F000;
const CB_CLONE_CBA2R: usize = 0x0;
const CB_CLONE_CBAR: usize = 0x4;
const CB_CLONE_SCTLR: usize = 0x8;

struct Radio {
    ce: ce::CeEngine,
    htc: htc::HtcState,
    wmi: wmi::WmiState,
    htt: htt::HttState,
    scan: scan::ScanState,
    sta: sta::StaState,
    ms: u32,
    api_ready: bool,
}

static API_READY: AtomicBool = AtomicBool::new(false);
static API_SCAN_ASKED: AtomicBool = AtomicBool::new(false);
static API_SCAN_DONE: AtomicU32 = AtomicU32::new(0);
static API_JOIN_ASKED: AtomicBool = AtomicBool::new(false);
static API_LEAVE_ASKED: AtomicBool = AtomicBool::new(false);

static mut RADIO_PTR: *mut Radio = core::ptr::null_mut();
static mut READY_HOOK: Option<fn()> = None;

fn get_radio() -> Option<&'static mut Radio> {
    let p = unsafe { RADIO_PTR };
    if p.is_null() { None } else { Some(unsafe { &mut *p }) }
}

fn get_radio_ref() -> Option<&'static Radio> {
    let p = unsafe { RADIO_PTR };
    if p.is_null() { None } else { Some(unsafe { &*p }) }
}

fn smmu_identity(smmu: &Mmio) {
    let sid_off = (WLAN_SID as usize & 0xFF) * 4;
    smmu.write(0x10800 + sid_off, 0x80000001);
    smmu.write(0x10C00 + sid_off, 0x00000001);
    let cb = CB_CLONE_OFF;
    smmu.write(cb + CB_CLONE_CBA2R, 0x00000001);
    smmu.write(cb + CB_CLONE_CBAR, 0x0001F000);
    smmu.write(cb + CB_CLONE_SCTLR, 0x000000E0);
    crate::log!("SWEET WLAN: SMMU identity for SID {:#x}", WLAN_SID);
}

fn radio_init_heap() -> bool {
    let smmu = match Mmio::map(WLAN_SMMU_PA, WLAN_SMMU_SZ) {
        Some(s) => s,
        None => return false,
    };
    smmu_identity(&smmu);

    let ce = match ce::CeEngine::start() {
        Some(c) => c,
        None => return false,
    };
    let p: *mut Radio = xnu::kalloc::<Radio>();
    if p.is_null() {
        crate::log!("SWEET WLAN: kalloc Radio failed");
        return false;
    }
    unsafe {
        addr_of_mut!((*p).ce).write(ce);
        addr_of_mut!((*p).htc).write(htc::HtcState::new());
        addr_of_mut!((*p).wmi).write(wmi::WmiState::new());
        addr_of_mut!((*p).htt).write(htt::HttState::new());
        // scan and sta are large (~120KB combined); kalloc zeroed them,
        // just fix the few non-zero defaults
        (*p).sta.bintval = 100;
        (*p).sta.dtim = 1;
        RADIO_PTR = p;
    }
    true
}

static mut POLL_FRAMES: [([u8; 2048], usize, usize); 16] = [([0; 2048], 0, 0); 16];

fn radio_poll(r: &mut Radio) {
    let ms = r.ms;
    let frames = unsafe { &mut *addr_of_mut!(POLL_FRAMES) };
    let mut nframes = 0usize;

    r.ce.poll(|c, data| {
        if nframes < 16 && data.len() <= 2048 {
            frames[nframes].0[..data.len()].copy_from_slice(data);
            frames[nframes].1 = c;
            frames[nframes].2 = data.len();
            nframes += 1;
        }
    });

    for i in 0..nframes {
        let c = frames[i].1;
        let len = frames[i].2;
        let raw = &frames[i].0[..len];
        let result = r.htc.rx(&mut r.ce, c, raw);
        if let Some((eid, hdr_off, plen)) = result {
            let payload = &frames[i].0[hdr_off..hdr_off + plen];
            if eid == r.htc.wmi_eid {
                let ev = r.wmi.rx(&mut r.htc, &mut r.ce, payload);
                match ev {
                    Some(wmi::WmiEvent::Ready) => {
                        r.htt.start(&mut r.htc, &mut r.ce, ms);
                    }
                    Some(wmi::WmiEvent::ScanEvent(body)) => {
                        if let Some((ev, _, _)) = wmi::scan_event_parse(body) {
                            r.scan.on_scan_event(ev, r.htt.msdus);
                        }
                    }
                    Some(wmi::WmiEvent::MgmtRx { freq, rssi, frame }) => {
                        r.scan.mgmt_rx(freq, rssi, frame, ms);
                        let our_mac = r.wmi.mac;
                        r.sta.on_mgmt_rx(&mut r.htc, &mut r.ce, frame, &our_mac);
                    }
                    Some(wmi::WmiEvent::VdevStartResp { status }) => {
                        r.sta.on_vdev_start_resp(status);
                    }
                    _ => {}
                }
            } else if eid == r.htc.htt_eid {
                r.htt.rx(&mut r.htc, &mut r.ce, payload, ms);
            }
        }
    }

    r.htc.drive(&mut r.ce);
    r.htt.poll(&mut r.htc, &mut r.ce, ms);

    if r.scan.scan_started && !r.api_ready {
        r.api_ready = true;
        API_READY.store(true, Ordering::Release);
        crate::log!("SWEET WLAN: radio ready for the rest of the kernel");
        let hook = unsafe { *addr_of_mut!(READY_HOOK) };
        if let Some(h) = hook {
            h();
        }
    }

    if r.scan.scan_started && !r.scan.scanning && API_SCAN_ASKED.load(Ordering::Acquire) {
        API_SCAN_ASKED.store(false, Ordering::Release);
        r.scan.next_scan_ms = 0;
        r.scan.scanning = r.wmi.scan_start(&mut r.htc, &mut r.ce);
        if !r.scan.scanning {
            r.scan.scans_done += 1;
            API_SCAN_DONE.store(r.scan.scans_done, Ordering::Release);
        }
    }

    if r.scan.scan_started && !r.scan.scanning && r.scan.next_scan_ms != 0 && ms >= r.scan.next_scan_ms {
        r.scan.next_scan_ms = 0;
        r.scan.scanning = r.wmi.scan_start(&mut r.htc, &mut r.ce);
    }

    if r.scan.scans_done > 0 {
        API_SCAN_DONE.store(r.scan.scans_done, Ordering::Release);
    }

    if r.wmi.ready && r.htt.phase == 2 && !r.scan.scan_started {
        r.scan.scan_started = true;
        r.scan.scanning = r.wmi.scan_start(&mut r.htc, &mut r.ce);
        crate::log!("SWEET WLAN: first scan {}", if r.scan.scanning { "started" } else { "FAILED" });
    }

    if API_JOIN_ASKED.load(Ordering::Acquire) {
        API_JOIN_ASKED.store(false, Ordering::Release);
        r.sta.join_asked = true;
    }
    if API_LEAVE_ASKED.load(Ordering::Acquire) {
        API_LEAVE_ASKED.store(false, Ordering::Release);
        r.sta.leave_asked = true;
    }
    let our_mac = r.wmi.mac;
    r.sta.drive(&mut r.htc, &mut r.ce, &mut r.htt, &r.scan, &our_mac);
    r.sta.pump_tx(&mut r.htc, &mut r.ce, &mut r.htt, &our_mac);
}

pub extern "C" fn wlan_thread(_arg: usize) {
    crate::log!("SWEET WLAN: thread started");

    if !crypto::selftest() {
        crate::log!("SWEET WLAN: crypto self-test FAILED, aborting");
        return;
    }

    if !radio_init_heap() {
        crate::log!("SWEET WLAN: radio init failed");
        return;
    }
    crate::log!("SWEET WLAN: radio initialized, entering poll loop");

    loop {
        let r = match get_radio() {
            Some(r) => r,
            None => break,
        };
        radio_poll(r);
        r.ms += 10;
        if r.ms % 10000 == 0 {
            crate::log!("SWEET WLAN: alive ms={} ready={} scans={}", r.ms, r.api_ready, r.scan.scans_done);
        }
        xnu::sleep_ms(10);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_ready() -> bool {
    API_READY.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_address(addr: *mut u8) {
    let mac = match get_radio_ref() {
        Some(r) => r.wmi.mac,
        None => wmi::OUR_MAC,
    };
    unsafe {
        core::ptr::copy_nonoverlapping(mac.as_ptr(), addr, 6);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_scan() -> i32 {
    if !API_READY.load(Ordering::Acquire) {
        return 6;
    }
    let before = API_SCAN_DONE.load(Ordering::Acquire);
    API_SCAN_ASKED.store(true, Ordering::Release);
    for _ in 0..300 {
        if API_SCAN_DONE.load(Ordering::Acquire) != before {
            return 0;
        }
        xnu::sleep_ms(100);
    }
    60
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_heard(index: u32, out: *mut scan::WlanBss) -> bool {
    let r = match get_radio_ref() {
        Some(r) => r,
        None => return false,
    };
    match r.scan.fill_wlan_bss(index as usize, r.ms) {
        Some(bss) => {
            unsafe { core::ptr::write(out, bss) };
            true
        }
        None => false,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_join(
    ssid: *const u8,
    ssid_len: u32,
    bssid: *const u8,
    pass: *const u8,
    pass_len: u32,
) -> i32 {
    if !API_READY.load(Ordering::Acquire) {
        return 6;
    }
    if ssid_len == 0 || ssid_len > 32 {
        return 22;
    }
    let r = match get_radio() {
        Some(r) => r,
        None => return 6,
    };
    let sl = ssid_len as usize;
    let ssid_slice = unsafe { core::slice::from_raw_parts(ssid, sl) };
    let pmk = if pass_len > 0 {
        let pass_slice = unsafe { core::slice::from_raw_parts(pass, pass_len as usize) };
        crypto::pbkdf2_pmk(pass_slice, ssid_slice)
    } else {
        [0u8; 32]
    };

    r.sta.ssid[..sl].copy_from_slice(ssid_slice);
    r.sta.ssid_len = sl as u8;
    if !bssid.is_null() {
        r.sta.bssid.copy_from_slice(unsafe { core::slice::from_raw_parts(bssid, 6) });
    } else {
        r.sta.bssid = [0; 6];
    }
    r.sta.pmk = pmk;

    let before = r.sta.join_done;
    API_JOIN_ASKED.store(true, Ordering::Release);
    for _ in 0..300 {
        let r = match get_radio_ref() {
            Some(r) => r,
            None => return 6,
        };
        if r.sta.join_done != before {
            return r.sta.join_result;
        }
        xnu::sleep_ms(100);
    }
    60
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_join_pmk(
    ssid: *const u8,
    ssid_len: u32,
    bssid: *const u8,
    pmk: *const u8,
) -> i32 {
    if !API_READY.load(Ordering::Acquire) || ssid_len == 0 || ssid_len > 32 {
        return if API_READY.load(Ordering::Acquire) { 22 } else { 6 };
    }
    let r = match get_radio() {
        Some(r) => r,
        None => return 6,
    };
    let sl = ssid_len as usize;
    r.sta.ssid[..sl].copy_from_slice(unsafe { core::slice::from_raw_parts(ssid, sl) });
    r.sta.ssid_len = sl as u8;
    if !bssid.is_null() {
        r.sta.bssid.copy_from_slice(unsafe { core::slice::from_raw_parts(bssid, 6) });
    } else {
        r.sta.bssid = [0; 6];
    }
    r.sta.pmk.copy_from_slice(unsafe { core::slice::from_raw_parts(pmk, 32) });

    let before = r.sta.join_done;
    API_JOIN_ASKED.store(true, Ordering::Release);
    for _ in 0..300 {
        let r = match get_radio_ref() {
            Some(r) => r,
            None => return 6,
        };
        if r.sta.join_done != before {
            return r.sta.join_result;
        }
        xnu::sleep_ms(100);
    }
    60
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_leave() -> i32 {
    if get_radio_ref().is_none() {
        return 6;
    }
    API_LEAVE_ASKED.store(true, Ordering::Release);
    for _ in 0..100 {
        let r = match get_radio_ref() {
            Some(r) => r,
            None => return 6,
        };
        if r.sta.phase == sta::StaPhase::Idle {
            return 0;
        }
        xnu::sleep_ms(100);
    }
    60
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_joined() -> bool {
    matches!(get_radio_ref(), Some(r) if r.sta.phase == sta::StaPhase::Up)
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_transmit(frame: *const u8, length: usize) -> bool {
    let r = match get_radio() {
        Some(r) => r,
        None => return false,
    };
    let data = unsafe { core::slice::from_raw_parts(frame, length) };
    r.sta.enqueue_tx(data)
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_set_ready_hook(hook: Option<fn()>) {
    unsafe { (*addr_of_mut!(READY_HOOK)) = hook };
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_set_rx_hook(hook: Option<extern "C" fn(*const u8, usize)>) {
    if let Some(r) = get_radio() {
        r.sta.rx_hook = hook;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sweet_wlan_set_link_hook(hook: Option<extern "C" fn(bool)>) {
    if let Some(r) = get_radio() {
        r.sta.link_hook = hook;
    }
}
