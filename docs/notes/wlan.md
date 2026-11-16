# WLAN / WiFi Stack (WCN3990 / ath10k)

## Status (2026-11-16)

Rust WLAN radio stack implemented in `skyios-kernel/src/wlan/` (8 modules, 0 errors, 34 warnings).
Activated by boot-arg `sweetwifi=1`.

### Modules

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | ~385 | Radio struct, SMMU identity mapping, poll loop, C API exports |
| `ce.rs` | ~230 | Copy Engines (12 CEs at PA 0x0C800000+0x240000) |
| `htc.rs` | ~160 | HTC protocol: phase-based state machine (0-6), connect HTT_DATA then WMI_CONTROL |
| `wmi.rs` | ~307 | WMI TLV protocol: SERVICE_READY -> INIT -> READY, scan commands across 37 frequencies |
| `htt.rs` | ~294 | HTT data path: RX ring (256 entries x 2048B), TX (64 slots x 2048B), frag desc bank |
| `scan.rs` | ~218 | BSS table (64 entries), beacon/probe parsing |
| `sta.rs` | ~500 | Station state machine: Idle -> Start -> Auth -> Assoc -> FourWay -> Up, WPA2-PSK 4-way handshake |
| `crypto.rs` | ~230 | SHA-1, HMAC-SHA1, PBKDF2 (4096 iter), PRF-SHA1, AES-128/unwrap, selftest |

### C API (exported to XNU)

- `sweet_wlan_ready()` -> bool
- `sweet_wlan_scan()` -> i32 (blocks up to 30s)
- `sweet_wlan_heard(index, out)` -> bool (fills WlanBss struct)
- `sweet_wlan_join(ssid, ssid_len, bssid, pass, pass_len)` -> i32
- `sweet_wlan_join_pmk(ssid, ssid_len, bssid, pmk)` -> i32
- `sweet_wlan_leave()` -> i32
- `sweet_wlan_joined()` -> bool
- `sweet_wlan_transmit(frame, length)` -> bool
- `sweet_wlan_address(addr)` -> void (copies 6-byte MAC)
- `sweet_wlan_set_ready_hook(hook)` -> void

### Dependencies / Blockers

- **WLFW QMI**: WCN3990 firmware is loaded by the WLFW QMI service (part of modem subsystem). Sweet has no modem implementation yet, so the firmware load path is not exercised.
- **en0 / Apple80211 XNU patch**: Needed to connect the Rust radio to XNU's networking stack (BSD ifnet + Apple80211 ioctl interface for wifid). Patch 0115 in progress.
- MAC address: locally administered `02:53:57:45:45:54` ("SWEET")

### Source

Ported from ios-topaz C++ ath10k implementation, rewritten in Rust (`#![no_std]`, edition 2024).
Uses `xnu.rs` FFI: `Mmio`, `Dma` (sub/copy_out/copy_in/zero/r64/pa), `spawn()`, `sleep_ms()`, `delay_us()`, `boot_arg_u32()`, `log!()`, `alloc_contig()`, `cache_clean/flush()`.

### Battery Patches (done)

- `xnu/patches/0062-sweet-touch-power-battery.patch` — touch, power key, battery stub
- `xnu/patches/0114-sweet-battery-gauge-charger.patch` — BQ27541 gauge + SMB1396 charger
