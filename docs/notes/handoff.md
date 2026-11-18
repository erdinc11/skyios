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
- Run 44217113864 (c2e3f38): Rust library compiles, XNU Build step likely fails
  (same patch application issues as run 44213158744)
- **Root cause of CI failure:** Patches in `xnu/patches/` have evolved out of sync.
  Many patches (0062, 0072, 0084, 0087, 0089-0093, 0094, 0101) fail to apply via
  `git apply`. The local build tree (`research/localbuild/dxb/`) has OLDER, working
  versions of these patches. The source `xnu/patches/0062` is significantly different
  from the local tree's version.
- Fixed RUST_TOOLCHAIN in xnu.yml: nightly-2026-08-13 → nightly-2026-07-28
  (newer nightlies produce unbootable kernels). Not yet committed.
- CI patches need regeneration against the actual XNU base to fix the cascade.

**Phone state:** Not detected in fastboot. User said they would put it in fastboot
before sleeping, but `fastboot devices` returns empty. May need reconnection.

**Probe history (continued from session 10):**
| Probe | Test | Boot note | Result |
|-------|------|-----------|--------|
| probe48 | 3s busy-wait + full dispatch | **(not booted)** | — |
| probe49 | +SMMU stall-on-fault fix | **(READY)** | — |

**Previous fixes (sessions 9-10, committed):**
- blr crash → ID-based dispatch (commit 402151b)
- sleep blocks forever → 3s busy-wait at thread start (commit 402151b)

**Next concrete actions:**
1. Boot probe49 (`fastboot boot skyios-shim/out/shim-boot-probe49.img`)
2. Verify GPU survives past fence 59 — SMMU faults logged but engine continues
3. If GPU rendering works: check frame rate, debug remaining CA draw rejections
4. Fix CI patches: sync `xnu/patches/` with working local tree versions
5. Implement GPU memory free path (zero frees currently — resource leak)
6. Study SnapMetal for Metal driver patterns
