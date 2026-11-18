# iOS 16 on sweet (Redmi Note 10 Pro, SM7150) - working notes

## Agent handoff / active work (2026-11-18, latest)

### >>> HANDOFF 2026-11-18 (session 11): SMMU stall-on-fault fix, probe49 built <<<

**Goal:** Get GPU rendering at 60fps. Previous blockers (blr crash, sleep) solved.
Current blocker: SMMU fault at fence 59 kills GPU present permanently.

**Root cause 3 — SMMU stall-on-fault (SOLVED, committed c2e3f38):**
- SCTLR CFCFG=1 (bit 5) in `mem_setup()` caused the SMMU to STALL GPU transactions
  on translation faults instead of aborting. When a user IB referenced a bad address
  (FAR=0x1400000528000, FSR=0x402), the SMMU stalled the transaction, the CP hung,
  the fence never arrived, `idle()` timed out at 1s, and `dead=true` was set permanently.
- **Fix:** Clear CFCFG (bit 5) in SCTLR write → terminate/abort mode. GPU gets DECERR
  for bad addresses, draws complete with garbage, CP continues. Added `fault_count`
  tracking and `check_clear_smmu_fault()` method to detect, log, and W1C-clear faults.
  Rewrote `idle()` to check/clear fault after success, retry once on timeout before
  setting dead=true.
- **Files changed (committed):** `skyios-kernel/src/cp.rs`
  - `mem_setup()`: SCTLR from `(1<<6)|(1<<5)|(1<<2)|(1<<1)|1` to `(1<<6)|(1<<2)|(1<<1)|1`
  - `Engine` struct: added `fault_count: u32`
  - New method `check_clear_smmu_fault()` — reads FSR/FAR/FSYNR, logs, W1C clears FSR
  - Rewrote `idle()` — checks fault after success, retries once on timeout
- **Commit:** c2e3f38 "kernel: fix SMMU stall-on-fault killing GPU after user IB faults"

**Bad texture address investigation (ongoing):**
- FAR=0x1400000528000 → FAR_HI=0x00014000, FAR_LO=0x00528000
- For 32-bit IOVA space (T0SZ=32), FAR_HI should be zero. GPU generated a >32-bit address.
- Likely from a texture descriptor: TEX_CONST_4=0x00528000 (bare buffer offset without
  0xC0000000 IOVA base), TEX_CONST_5 bits[0:16] contain garbage 0x4000.
- SkyRender.m's `textureDescriptor` and `resolveTexGpu` look correct for valid paths.
  Draw path skips when desc=0 or prog invalid. The fault may come from a race or
  uninitialized descriptor reaching the GPU before the skip check.
- With SMMU abort mode, these faults are no longer fatal — just logged.

**Local build flow (re-link only, ~2 seconds):**
1. `TOOLCHAIN=nightly-2026-07-28 skyios-kernel/build.sh` → builds Rust lib
2. Run the linker command from the build directory (see session transcript or
   `research/localbuild/build10.log` line 1865) pointing to new Rust lib
3. `xcrun strip -x` the unstripped kernel
4. `skyios mkpayload KERNEL ADT OUT --cmdline "..." --ramdisk dtb/boot_unpacked/ramdisk`
5. `skyios-shim/build.sh PAYLOAD OUT.img`

**Boot images:**
- `skyios-shim/out/shim-boot-probe48.img` — blr+sleep fixes, NO SMMU fix
- `skyios-shim/out/shim-boot-probe49.img` — blr+sleep+SMMU fixes (NEW)
- Args: `sweetgpu=5 sweetgpu_delay=20 sweetaccel=2 sweetmetal_procs=skyios-accel-test,backboardd sweetdyldvars=1 sweetinsert=SpringBoard:/usr/lib/skyios/SkyPBFix.dylib sweetwifi=1 sweetbattery=1 sweetreboot=120 sweetshot=90`

**CI build status:**
- **Run 44227242600 (509da77, SUCCESS):** All patches applied, kernel linked
  successfully. All 4 previously-undefined symbols resolved:
  sweet_rs_init (0x...92288), sweet_rs_spawn_id (0x...92554),
  sweet_rs_kalloc (0x...9283c), sweet_sta_init (0x...fabe0).
  Artifact downloaded, probe50 boot image built.
  **probe50 = CI kernel with all C fixes (id-dispatch, 3s busy-wait,
  tsleep, SMMU abort, WLAN stubs) + Rust GPU driver.**
- **Run 44225287511 (38fe45c, FAILED):** 0093a SKIPPED — bsd/conf/files
  hunk failed at line 108 (context didn't account for 0072's additions).
  0115 iokit hunk now works (sweet_sta_init resolved). Still had 3
  undefined symbols from 0093a skip.
- **Run 44222102669 (9e5ed71, FAILED):** 4 undefined linker symbols.
- **Commits this session (12-14):**
  - 509da77: Fixed 0093a bsd/conf/files hunk context (session 14)
  - 38fe45c: Fixed 0115 iokit hunk line number (24→23) (session 13)
  - 3260445: Added 0093a patch + fixed 0115 iokit context (session 13)
  - 0699bf9: Updated handoff with CI session 12 progress
  - 9e5ed71: Fixed 0115 line counts (session 12)
  - 661cbaf: Restored 0062 documentation comments (session 12)
  - 1df973a: Fixed 0062 diff structure (session 12)
  - 2217cc7: Fixed RUST_TOOLCHAIN (session 12)
- Remaining skipped dxb patches (expected, not ours): iobuffermemd_monterey,
  skywalk_sonoma, syntax_checker_sonoma, TrustCache (files already exist).
- **SnapMetal research (session 11):** Studied the MIT-licensed Metal driver at
  github.com/Snapintosh/SnapMetal. Key findings for our 60fps target:
  - Uses IOGPU framework for submission (vs our custom IOKit user client)
  - Per-draw state emission is much richer: LRZ, bindless descriptors,
    framebuffer-fetch barriers, driver constants, 48+ register groups
  - Has proper IB packaging with kext-side relocation patching
  - CA renderer test (harness/ca-renderer.m) captures command streams, not pixels
  - Targets A8XX (Adreno X2-90), not directly applicable to our A6XX registers

**Phone state:** Booting probe53 (or in fastboot for next boot).
Boot command: `fastboot boot skyios-shim/out/shim-boot-probe53.img`

**Probe history (continued from session 10):**
| Probe | Test | Boot note | Result |
|-------|------|-----------|--------|
| probe48 | 3s busy-wait + full dispatch | **(not booted)** | — |
| probe49 | +SMMU stall-on-fault fix | **(READY, local build)** | — |
| probe50 | CI kernel, cpus=1 | Booted but console text covers screen, too slow | Console only |
| probe51 | CI kernel, cpus=8 | **BOOT SUCCESS** — log confirms backboardd, 15 GPU presents, lock screen behind console text | GPU works, console covers GUI |
| probe52 | Fresh ADT, gpu_delay=10 | ADT missing props, boot stalls early | Failed |
| probe53 | Original ADT, reboot=300, no kprintf_spam | **(READY)** | — |

**probe51 boot results (logs/boot-probe51.txt, 4503 lines):**
- 8/8 CPUs alive, L3 1459 MHz, silver 1.8 GHz, gold 2.3 GHz
- Battery: PM6150 100% 4453mV, external=1 charging=0 (POWER_PATH fix works)
- GPU: Adreno 618 @ 430 MHz, 15 GPU presents, 0/64 bad pixels, ~2.1ms/frame
- Metal: backboardd connected, CA render passes at 1125×2436
- WLAN: thread started, crypto selftest OK, but radio_init_heap silent (no "radio initialized" or "radio init failed")
- USB: DWC3 up, ECM gadget did not connect to host
- All prog=3 textured draws skipped (desc=0, texture descriptor empty)
- Console text stayed on screen because GPU delay was 60s (from ADT stale args) and user rebooted before presents cleared it

**Key issue: stale ADT boot-args override cmdline.** The `sweet-0081hi.adt`
contains `sweetgpu_delay=60 sweettop=5 sweetautobahn=1` etc. that
`PE_parse_boot_argn` finds first. Cmdline args come after. Need to
strip stale boot-args from the ADT or make mkpayload put cmdline first.

**Previous fixes (sessions 9-10, committed):**
- blr crash → ID-based dispatch (commit 402151b)
- sleep blocks forever → 3s busy-wait at thread start (commit 402151b)
- Battery stuck-charging → POWER_PATH cross-check (commit 899cbb5)

**Next concrete actions:**
1. ~~Fix CI patches~~ — DONE
2. ~~Build + boot CI kernel~~ — DONE (probe51 confirmed working)
3. Fix stale ADT boot-args (strip or rebuild sweet-0081hi.adt without embedded args)
4. Boot probe53 and wait full 2-3 min for lock screen to appear
5. Get USB Ethernet working for live log capture
6. Debug prog=3 texture descriptor (desc=0 → all textured draws skipped)
7. Debug WLAN radio_init_heap hang (passes crypto selftest, then silent)
8. Implement GPU memory free path (zero frees currently — resource leak)
