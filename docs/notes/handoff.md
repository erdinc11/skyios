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

**Phone state:** Not detected in fastboot. User said they would put it in fastboot
before sleeping, but `fastboot devices` returns empty. May need reconnection.
When phone is reconnected: `fastboot boot skyios-shim/out/shim-boot-probe50.img`

**Probe history (continued from session 10):**
| Probe | Test | Boot note | Result |
|-------|------|-----------|--------|
| probe48 | 3s busy-wait + full dispatch | **(not booted)** | — |
| probe49 | +SMMU stall-on-fault fix | **(READY, local build)** | — |
| probe50 | CI kernel: all C fixes + Rust GPU | **(READY, CI run 44227242600)** | — |

**Previous fixes (sessions 9-10, committed):**
- blr crash → ID-based dispatch (commit 402151b)
- sleep blocks forever → 3s busy-wait at thread start (commit 402151b)

**Next concrete actions:**
1. ~~Fix CI patches~~ — DONE, all applied, kernel built (CI run 44227242600)
2. ~~Build probe50~~ — DONE (`skyios-shim/out/shim-boot-probe50.img`, 19MB)
3. Reconnect phone USB (not detected in fastboot — cable may need replug)
4. Boot probe50 (`fastboot boot skyios-shim/out/shim-boot-probe50.img`)
5. Verify GPU survives past fence 59 — SMMU faults logged but engine continues
6. If GPU rendering works: check frame rate, debug remaining CA draw rejections
7. Implement GPU memory free path (zero frees currently — resource leak)
8. Add more texture formats to resolveTexGpu (only BGRA8/RGBA8 currently)
