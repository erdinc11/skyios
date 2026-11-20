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

IDs 27+ are reserved for fine-grained UFS/HFS, display, and GPU stages. Add a registry row before assigning each ID; never reuse an ID.
