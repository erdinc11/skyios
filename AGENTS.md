# Working on SkyIOS

- Read `NOTES.md` before starting; it contains the project goal, hardware facts,
  current blocker, local artifact locations and the next concrete action.
- The user switches agents frequently. Update the active handoff in `NOTES.md`
  after each milestone and before stopping. Record commits, CI run IDs, exact
  build/boot results and unfinished work. Distinguish observations from guesses.
- Commit completed changes. Use `gh` for GitHub Actions kernel builds and logs.
  Local single-file cross-compiles are useful before spending a CI run.
- Device bring-up uses temporary `fastboot boot` images. Preserve the Android
  installation and partition backups. The user approved (2026-10-15) using
  internal UFS for iOS and said Android may be deleted: the iOS root goes
  into the existing `userdata` partition (no GPT changes). Keep bootloaders,
  recovery, modem, persist and the backups intact, and confirm each
  destructive step with the user before running it.
- Keep device dumps, serial numbers and downloaded Apple artifacts out of git.

## Active device bring-up mission

- The active priority is a verified iOS boot with real Adreno 618 rendering; do not start secondary phone-function work until that is verified on the device. iMessage, iCloud, Apple ID, and Apple Pay are out of scope.
- For every technical failure or change, follow the official Superpowers `systematic-debugging` phases: investigate root cause, compare working patterns, test one falsifiable hypothesis at a time, then implement and verify. Use `verification-before-completion` before accepting any milestone. Never treat compilation, GPU initialization, framebuffer output, or CPU fallback as proof of GPU-rendered iOS.
- Before any device experiment, verify the automatic recovery path, rollback image, artifact identity, and evidence collection. Prefer temporary `fastboot boot`; preserve bootloader, recovery, modem, persist, calibration, and identity partitions. Never make an experiment depend on a person pressing buttons while the user is unavailable.
- Keep automatic recovery/watchdogs enabled and short. Increase a timeout only with evidence that it alone cuts off an otherwise progressing boot. Do not run concurrent device-control operations.
- Use the live camera at `http://192.168.1.31:8080/` together with ADB/Fastboot, logs, and host timestamps. Distinguish stale persistent logs from current-boot evidence.
- Maintain stable, unique numerical boot checkpoint IDs mapped to exact source/function boundaries in the repository. Show large red, safe-inset IDs on the framebuffer when available; use early logging before display is available. Never reuse IDs silently.
- Keep `progress.md` current with observed state, exact evidence, hypotheses, changes, artifact hashes, recovery status, and the next justified action. Preserve failed experiments and distinguish fact from hypothesis.
- After compaction or a new agent session, reread the active mission, this file, `NOTES.md`/handoff, `progress.md`, and the current debugging skill before acting. Continue from the latest verified evidence rather than repeating old experiments.
