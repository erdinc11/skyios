# iOS 16 on sweet (Redmi Note 10 Pro, SM7150) - working notes

## Agent handoff / active work (2026-11-18, latest)

### >>> HANDOFF 2026-11-18 (session 10): blr crash solved, sleep bug re-discovered, probe48 ready <<<

**Goal:** Get GPU rendering at 60fps. Immediate blocker: Rust-spawned threads crash.

**Root cause 1 — blr crash (SOLVED):**
- Indirect function pointer calls (`blr x21`) from kernel threads to Rust code crash.
- Direct calls (`bl _symbol`) to the same Rust functions work.
- **Fix:** ID-based dispatch. C-side `sweet_rs_spawn_id(id, arg)` → `sweet_rs_thread()`
  calls `sweet_rs_thread_dispatch(id, arg)` via direct `bl`. Rust-side match dispatches
  to gpu_thread/reboot_thread/wdog_thread/wlan_thread.
- Files changed: `sweet_rs.c` (struct uses id+arg, not fn pointer), `lib.rs` (dispatch fn),
  `xnu.rs` (spawn_id FFI).
- **Proof:** probe46 boot note `A; CR; T; TX; T; TX; T; TX; Z;` — all 3 threads enter
  and return from `sweet_rs_thread_dispatch` via `bl`.

**Root cause 2 — sleep blocks forever (RE-DISCOVERED):**
- Both `delay_for_interval()` AND `tsleep()` block forever in threads spawned from
  `sweet_rs_init()` context. The previous session's tsleep fix (memory says it worked
  in probe30) does NOT work for threads created during sweet_rs_init.
- sweet_log_start / sweet_sample_start threads (called right after sweet_rs_init in
  sweet_ufs_rootdev) work fine with tsleep — the difference is timing.
- **Evidence:** probe46 C-side reboot thread used tsleep for 120s — never fired.
  Phone ran indefinitely without rebooting.
- **Fix (probe48, ready to boot):** 3-second busy-wait (mach_absolute_time + yield)
  at thread start in `sweet_rs_thread()`, before calling Rust dispatch. By then, boot
  has progressed past sweet_log_start/sweet_sample_start and sleep infrastructure works.
- C-side reboot thread also uses busy-wait (mach_absolute_time + yield).

**Probe history this session:**
| Probe | Test | Boot note | Result |
|-------|------|-----------|--------|
| probe42 | First ID dispatch (no markers) | `A; Z;` | No GPU output — dispatch silent |
| probe43 | +T/TX markers in C wrapper | `A; T; T; Z; T;` | Threads start, no TX (crash in dispatch) |
| probe44 | if/else + xnu::log in dispatch | (empty) | No boot note at all — Rust rebuild issue? |
| probe45 | +printf in sweet_rs_init | `A; T; T; T; Z;` | 3 threads spawn, 0 return |
| probe46 | No-op dispatch (just log+return) | `A; CR; T; TX; T; TX; T; TX; Z;` | ALL threads return! bl works! |
| probe47 | sleep_ms test in dispatch | (not booted) | — |
| probe48 | 3s busy-wait + full dispatch | **(READY)** | — |

**What changed (files, not committed):**
- `research/localbuild/dxb/xnu/osfmk/arm64/sweet_rs.c`:
  - ID-based dispatch (struct sweet_rs_job has id+arg, not fn pointer)
  - `sweet_rs_thread()` busy-waits 3s via mach_absolute_time before calling Rust dispatch
  - `sweet_rs_spawn_id()` function
  - `sweet_rs_spawn()` stub (returns false, link compat)
  - C-side reboot thread (`sweet_c_reboot_thread`) with busy-wait + PSCI reset
  - `sweet_rs_init()` spawns C-side reboot thread for sweetreboot arg
- `skyios-kernel/src/lib.rs`:
  - `sweet_rs_thread_dispatch(id, arg)` with if/else chain (not match — avoids jump table)
  - Thread ID constants THREAD_GPU=1, THREAD_REBOOT=2, THREAD_WDOG=3, THREAD_WLAN=4
  - `sweet_rs_start()` calls `xnu::spawn_id()` instead of `xnu::spawn()`
  - gpu_thread uses `xnu::log(c"...")` instead of `crate::log!()` for first messages
- `skyios-kernel/src/xnu.rs`:
  - `sweet_rs_spawn_id` FFI declaration
  - `spawn_id()` public wrapper

**Boot image ready:** `skyios-shim/out/shim-boot-probe48.img`
- Args: `sweetgpu=5 sweetgpu_delay=20 sweetaccel=2 sweetmetal_procs=skyios-accel-test,backboardd sweetdyldvars=1 sweetinsert=SpringBoard:/usr/lib/skyios/SkyPBFix.dylib sweetwifi=1 sweetbattery=1 sweetreboot=120 sweetshot=90`
- C-side auto-reboot at 120s (busy-wait, doesn't depend on Rust dispatch)

**SnapMetal reference (found by user):**
- https://github.com/Snapintosh/SnapMetal — MIT-licensed Metal driver for Adreno X2-90
- Implements Metal device, command buffers, encoders on PM4 command streams via IOGPU
- Companion: SnapAdreno (kernel driver), air2ir3 (AIR→IR3 shader compiler)
- Has CA renderer test harnesses — directly relevant to our work
- Worth studying for Metal driver patterns and shader compilation approach

**Phone state:** Running probe46 (no-op dispatch). Needs manual reboot to fastboot
(Vol Down + Power) to boot probe48.

**Next concrete actions:**
1. Boot probe48 (`fastboot boot skyios-shim/out/shim-boot-probe48.img`)
2. If GPU thread works (SWEET GPU messages visible): the blr + sleep bugs are BOTH fixed
3. Study SnapMetal for Metal driver patterns and air2ir3 shader compiler
4. Debug GPU rendering pipeline for 60fps
5. Commit working changes

</content>
</invoke>