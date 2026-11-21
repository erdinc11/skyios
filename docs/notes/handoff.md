# iOS 16 on sweet (Redmi Note 10 Pro, SM7150) - working notes

### >>> HANDOFF 2026-11-21 04:05: CPU driver never matches despite valid nubs

- CI tip `ca579cc` (patches 0116, 0117, 0119, 0120, 0121; CI run `44847148421` in progress). Last downloaded kernel: `ci-artifacts/44845596719/kernel.development.sweet` SHA-256 `526a82d6…bec7ad90`.
- **Evidence chain:** (1) 0119 two-number marker: peak 62 / current 62 → no checkpoint above 62 was ever drawn; `SweetCPU::start` (ID 65) never ran. (2) 0120: peak 96 / current 91 → `processTopLevel` ran, `/cpus` was found, CPU nubs were created, and no CPU nub lacked a `logical-cpu-id` (ID 97 never drawn). (3) Therefore the failure is in IOKit matching/attach of the `SweetCPU` personality against the existing `IOPlatformDevice` CPU nubs, not in property validation or CPU registration.
- **0121** marks `IODTPlatformExpert::compareNubName` for `qcom,kryo470`: peak 99 = compared and matched, 98 = compared and failed, 96 = never consulted.
- Marker protocol: left dim-red = highest ID reached, right bright-red = most recent; rectangle panel (810,96) 190x104; registry in `docs/boot-checkpoints.md`.
- Device tools: `skyios-build/devctl.sh` + `device-supervisor.sh` (singleton launchd `com.skyios.device-supervisor`); every device-changing request must carry the active lease token; automatic recovery returns the verified serial `cd7c252c` from fastboot to TWRP after ~30 s without a lease.
- Next: read the 0121 peak. If 99, instrument attach/checkResources/probe; if 96, investigate why the personality is not considered; if 98, inspect the CPU nub properties seen by the compare.

### >>> HANDOFF 2026-11-20: root volume layout / automatic recovery <<<

**Active goal:** boot iOS on the Redmi Note 10 Pro and reach GPU-rendered UI.

- `auto-boot.sh` repeatedly boots `skyios/skyios-shim/out/shim-ci-0069adt-wd.img`;
  the CPU1 watchdog returns failed boots to TWRP. The phone's display still
  shows a D22AP panic screen, but the persistent ring pulled from TWRP at
  23:18:40 has later runtime output: `SweetPlatformExpert` started and
  registered, 6/6 CPUs came alive, and UFS mounted `userdata` read-write.
  Treat the old panic display as possibly stale; runtime evidence shows at
  least one boot passed platform setup.
- That log then failed root mount repeatedly with
  `hfs_ValidateHFSPlusVolumeHeader: unknown Volume Signature : 504d`.
  `ios16-root.img` and `ios16-root.simg` contain an Apple Partition Map at
  block 2; the HFS+ partition starts at block 64 (HFS+ header at file offset
  `0x8400`). The kernel mounts the whole `userdata` block device directly and
  reads `PM` at offset `0x400`, so this is the current concrete root blocker.
- Verified `ios16-bare.img` is the APM-free HFS+J volume (header `H+` at
  `0x400`), size 17,179,836,416 bytes. A read-only macOS mount confirmed the
  volume is `iOSRoot` and contains `/sbin/launchd`, `/System/Library`, and
  `/Applications`.
- TWRP ADB identifies the target as `/dev/block/sda34`, 114,970,046,464 bytes;
  the root image fits. `auto-boot.sh` now pulls the recovery log, writes
  `ios16-bare.img` to userdata via the existing `dd-root.sh` helper when in
  TWRP, reads back the first 4 KiB and compares it, records a marker after
  success, then continues to boot the kernel. The updated poller is running;
  at the 23:25:58 check ADB was in recovery and the next poll should perform
  this one-time write. No partition has been flashed yet.
- The local D22AP `IONameMatch` source adjustment is committed on the
  `codex/d22ap-platform-match` fork branch, but it is not the current blocker.
  CI run `44808833567` failed in `bsd_init.c:532` because `sweet_note` is
  undeclared; no kernel artifact was produced. The existing 0069 kernel is
  sufficient to test the raw HFS+ root next.

**Next:** let the active poller write and verify the APM-free root from TWRP,
then boot the existing watchdog image and read the new ring. Confirm that HFS+
mounts and launchd starts; fix the next observed issue, then enable the GPU
boot args and continue toward the rendered UI. Preserve the other uncommitted
shim/tool changes in the working tree.

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
