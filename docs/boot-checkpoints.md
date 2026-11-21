# Boot checkpoint registry

IDs are stable boundaries, not percentages. IDs 1–20 are the early XNU markers in patches 0010/0012; IDs 21–26 are added by patch 0013. A marker proves execution reached the named boundary only. It does not prove later initialization, user-visible usability, or GPU execution. The current renderer writes a two-digit red ID directly to the mapped framebuffer, independently of the Adreno.

| ID | Source / function boundary | Arrival proves | Does not prove | Next / dependency |
|---:|---|---|---|---|
| 01 | `osfmk/arm/arm_init.c`, `arm_init` entry | XNU entered C initialization with MMU on | platform expert or CPU setup | 02; bootstrap framebuffer |
| 02 | `arm_init`, after `PE_init_platform` | platform expert setup returned | device matching for every service | 03; platform expert |
| 03 | `arm_init`, after CPU topology parse | topology parser returned | secondary CPUs online | 04; device tree |
| 04 | `arm_init`, after `cpu_bootstrap` | boot CPU bootstrap returned | SMP | 05; CPU structures |
| 05 | `arm_init`, after `kernel_startup_bootstrap` | kernel startup bootstrap returned | VM completion | 06; scheduler/bootstrap |
| 06 | `arm_init`, after `cpu_init` | primary CPU init returned | other CPUs started | 07; timer/CPU setup |
| 07 | `arm_init`, immediately before `arm_vm_init` | VM bootstrap was entered | VM tables complete | 08–14; boot mappings |
| 08 | `sweet_early_relocate`, before framebuffer VA switch | old bootstrap mapping is still writable | new mapping valid | 09; bootstrap mapping |
| 09 | `sweet_early_relocate`, after setting framebuffer VA | kernel framebuffer VA selected | normal console initialized | 10; final framebuffer mapping |
| 10 | `arm_vm_init`, after TTBR0/TLB transition | transition instruction sequence returned | all physmap pointers valid | 11; translation tables |
| 11 | `arm_vm_init`, after physmap pointer rebinding | key table pointers rebound | pmap bootstrap complete | 12; physmap |
| 12 | `arm_vm_init`, before `pmap_bootstrap` | pmap bootstrap is next | pmap success | 13; memory map |
| 13 | `arm_vm_init`, after `pmap_bootstrap` | pmap bootstrap returned | all later VM setup | 14; pmap |
| 14 | `arm_vm_init`, after early I/O page-table loop | early I/O table loop completed | device drivers ready | 15; VM setup |
| 15 | `arm_init`, after `arm_vm_init` returns | VM init returned to caller | BSD init | 16; VM |
| 16 | `arm_init`, before `serial_init` | serial init is next | console initialized | 17; console setup |
| 17 | `pexpert/arm/pe_serial.c`, `fbcon_init`, immediately before `ml_io_map_wcomb` | framebuffer console config was read and mapping is next | mapping success | 18; display tree |
| 18 | `pe_serial.c`, immediately after `ml_io_map_wcomb` returns | framebuffer mapping call returned | correct visible scanout | 19; display mapping |
| 19 | `arm_init`, after `serial_init` | serial/video console init returned | BSD init | 20; console |
| 20 | `arm_init`, after early kprintf startup | early diagnostic output setup returned | root filesystem or launchd | 21; kernel startup |
| 21 | `bsd/kern/bsd_init.c`, `bsd_init` entry | BSD init began | root mount attempted | 22; VM and kernel threads |
| 22 | `bsd_init`, immediately before `vfs_mountroot` | root-mount call is next | mount success | 23 or 24; UFS device and filesystem |
| 23 | `bsd_init`, successful return from `vfs_mountroot` | root filesystem mounted | launchd started | 25; mounted root |
| 24 | `bsd_init`, failed return from `vfs_mountroot` | root mount returned an error | exact filesystem root cause | 22 retry; storage/filesystem |
| 25 | `bsd_init`, immediately before `bsd_utaskbootstrap` | user-task bootstrap is next | launchd reached `main` | 26; root mount |
| 26 | `bsd/kern/kern_exit.c`, accepted PID 1 write to `kern.initproc_spawned` | launchd reached `main` | SpringBoard or GPU acceleration | future userspace IDs; sysctl path |
| 46 | `osfmk/kern/startup.c`, after `kernel_startup_initialize_upto(STARTUP_SUB_OSLOG)` | all OSLOG-rank startup entries returned | telemetry, console, scheduler | 47; kernel_bootstrap |
| 47 | `startup.c`, after `telemetry_init()` | telemetry init returned | console initialization | 48; kprintf |
| 48 | `startup.c`, after `console_init()` | console init returned | framebuffer ownership by video driver | 49; stackshot |
| 49 | `startup.c`, after `stackshot_init()` | stackshot init returned | scheduler | 50 |
| 50 | `startup.c`, after `sched_init()` | scheduler init returned | MAC policy | 51 |
| 51 | `startup.c`, after `mac_policy_init()` | MAC policy init returned | MACH IPC startup | 52 |
| 52 | `startup.c`, after `kernel_startup_initialize_upto(STARTUP_SUB_MACH_IPC)` | MACH IPC-rank startup returned | kernel bootstrap thread | 53 |
| 53 | `startup.c`, `kernel_bootstrap_thread` entry | bootstrap thread is running on CPU0 | scheduler startup | 54 |
| 54 | `startup.c`, after `sched_startup()` | scheduler services started | thread daemon | 55 |
| 55 | `startup.c`, after `thread_daemon_init()` | thread daemon ready | thread calls | 56 |
| 56 | `startup.c`, after `initialize_screen(NULL, kPEAcquireScreen)` | screen acquisition path returned | display driver ownership | 57; video |
| 57 | `startup.c`, after `vm_commpage_init/commpage_text_init` | commpages initialized | MAC mach policy | 58 |
| 58 | `startup.c`, after `mac_policy_initmach`/`vnguard_policy_init` | vnguard policy initialized | trust cache | 59 |
| 59 | `startup.c`, after `load_static_trust_cache()` | trust cache startup returned | lockdown ranks | 60 |
| 60 | `startup.c`, after `kernel_startup_initialize_upto(STARTUP_SUB_LOCKDOWN)` | lockdown-rank startup returned | machine lockdown | 61 |
| 61 | `startup.c`, after `machine_lockdown()` | text/rodata lockdown returned | max CPU nail-down | 62 |
| 62 | `startup.c`, immediately before `kperf_init_early()` | kperf max-CPU wait is next | CPU registration | 63 or stall |
| 63 | `startup.c`, after `kperf_init_early()` | `ml_wait_max_cpus` returned; all processors accounted for | bsd_init | 64 |
| 64 | `startup.c`, immediately before `bsd_init()` | BSD init is next | checkpoints 21–26 | 21 |

IDs 27–45 remain reserved for fine-grained UFS/HFS, display, and GPU stages. Add a registry row before assigning each ID; never reuse an ID.

| 65 | `iokit/Kernel/arm/SweetCPU.cpp`, `SweetCPU::start` entry | the CPU driver was instantiated and matched | provider properties or topology | 66 |
| 66 | `SweetCPU::start`, `logical-cpu-id` present | the platform expert annotated the CPU nub | `reg` property | 67 |
| 67 | `SweetCPU::start`, `reg` property valid | nub has a 4-byte `reg` | logical range | 68 |
| 68 | `SweetCPU::start`, logical id in topology range | topology includes this CPU | phys id match | 69 |
| 69 | `SweetCPU::start`, nub phys id matches topology | nub and topology agree | `IOCPU::start`/`ml_processor_register` | 86 |
| 70 | `SweetCPU::start`, startAll thread spawned | boot CPU scheduled the SMP starter | startAll execution | 71 |
| 71 | `SweetCPU::startAll` entry | starter thread is running | all CPUs registered | 72 |
| 72 | `startAll`, all CPUs registered | every enabled CPU nub reached `start` | processor start loop | 73 |
| 73 | `startAll`, every processor started and alive | PSCI/SGI path succeeded for all | max CPU accounting | 74 |
| 74 | `startAll`, `ml_set_max_cpus` returned | max-CPU accounting was published | kperf wait exit | 63 |
| 75 | `SweetCPU::start` early return: no `logical-cpu-id` | nub annotation failed | — | investigate `ml_get_cpu_number` vs DT `reg` |
| 76 | `SweetCPU::start` early return: bad `reg` | nub property shape wrong | — | inspect DT property |
| 77 | `SweetCPU::start` early return: logical id out of range | nub id not in parsed topology | — | inspect topology count |
| 78 | `SweetCPU::start` early return: phys id mismatch | nub `reg` != topology phys id | — | inspect DT reg encoding |
| 81 | `SweetCPU::initCPU` entry | per-CPU init path ran | SGI self-test | 72/73 |
| 82 | `osfmk/arm/machine_routines_common.c`, `ml_wait_max_cpus` entry | kperf is waiting for max CPUs | max CPUs published | 83 or stall |
| 83 | `ml_wait_max_cpus` return | wait completed | `kperf_init_early` return | 63 |
| 84 | `ml_set_max_cpus` entry | publisher reached | flag published | 85 |
| 85 | `ml_set_max_cpus` completion | `max_cpus_initialized = SET` | wait wakeup | 83 |
| 86 | `SweetCPU::start`, before `ml_processor_register` | about to register processor | registration result | 87 |
| 87 | `SweetCPU::start`, after `ml_processor_register` returned success | processor registered | processor_start | 88 |
| 88 | `startAll`, before `processor_start` | about to start a processor | result | 89 |
| 89 | `startAll`, after `processor_start` returned success | processor start call succeeded | initCPU/alive | 81/72 |

IDs 90+ are reserved for the remaining `processTopLevel`/nub-creation diagnostics and userland/GPU stages if the above cannot distinguish the failure.

## Diagnostic latch (patch 0122)

A second persistent box at panel (810, 216), 120x72, shows **three-digit diagnostic IDs** (101–108), dim/bright red on black. It is independent of the two-number checkpoint ladder, so CPU-matching events are preserved regardless of draw order. The fbcon reserves both rectangles.

| ID | Location | Meaning |
|---:|---|---|
| 101 | `iokit/Kernel/IOCatalogue.cpp`, `findDrivers` | a `SweetCPU` personality was present in the candidate array for the provider class |
| 102 | `iokit/Kernel/IOService.cpp`, `doServiceMatch` | the `SweetCPU` candidate was reached before passive matching |
| 103 | `doServiceMatch`, after `matchPassive` succeeded | the `SweetCPU` candidate's IOClass/provider/name matching passed |
| 104 | `SweetCPU::start` entry | the driver's start was invoked |
| 105 | `SweetCPU::start`, after property checks | nub properties and topology validation passed |
| 106 | `SweetCPU::startAll` entry | the SMP starter thread ran |
| 107 | `startAll`, after `ml_set_max_cpus` | max-CPU accounting was published |
| 108 | reserved | start/registration rejection or timeout |

## Two-number marker (patch 0119)

The marker now draws two two-digit numbers side by side: the **left number in dim red is the highest checkpoint ID reached since boot**, the **right number in bright red is the most recent checkpoint**. The peak is retained because early checkpoints are overwritten within milliseconds; without it, a transient diagnostic (for example 74) is invisible once 62 is drawn. The rectangle moved to panel (810, 96), 190x104, digits at 5x scale. The fbcon still never writes inside the rectangle.

## Visible marker design (patch 0116)

- The marker is a 120×112 px box at panel (880, 96), two `iso_font` digits at 6× scale, red `0xffff2020` on black.
- `sweet_early_mark()` records the last stage and repaints the box even after the early console is disabled.
- The framebuffer console (`pexpert/arm/pe_serial.c`) never clears or draws pixels inside the marker rectangle, so console scrolling cannot erase the current checkpoint. `fbcon_init()` repaints it after clearing the screen.
- Marker drawing is independent of the Adreno/GPU and of the iOS compositor; it uses the mapped raw framebuffer only.
- Arrival at an ID proves only that the named boundary was reached. It does not prove the next boundary, nor that a subsystem is functional.

