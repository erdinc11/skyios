# SkyIOS Working Notes

Split from the monolithic NOTES.md. Read **handoff.md** first.

## Index

- [**handoff.md**](handoff.md) — Current agent handoff: GPU/Metal state, blockers, build process, how to continue
- [**gpu-milestones.md**](gpu-milestones.md) — GPU and Metal driver milestones (CA Metal renderer, first Metal API, user-space GPU, 3D triangle, blending, fb-fetch, IOSurface import, screenshots, wallpaper investigation, stock apps)
- [**hfi-repo.md**](hfi-repo.md) — HFI v1 boot sequence details and repo restructuring to Cargo workspace
- [**early-gpu.md**](early-gpu.md) — Early GPU milestones (GX power, cmd-db, os_log, PosterBoard crash)
- [**early-updates.md**](early-updates.md) — Pre-GPU updates (CPU clocks, Rust kernel, local XNU builds, touch, Settings, reinstall)
- [**gui-bringup.md**](gui-bringup.md) — Full GUI bringup chronicle: HANDOFF 10-17 state, phone/kernel workflow, patches 0046-0057, IOSurface/IOMFB reverse engineering, root images v2-v12, daemon fixes, all the way to iOS GUI on the phone
- [**session-log.md**](session-log.md) — Older session log and superseded handoffs
- [**hardware-design.md**](hardware-design.md) — QEMU emulation, repo layout, device access, hardware facts, XNU implications, kernel architecture
- [**wlan.md**](wlan.md) — WLAN/WiFi stack (WCN3990/ath10k in Rust), battery patches
