use crate::adt;
use crate::util::{cstr_at, die, parse, put_u32, put_u64, read, u16le, u32le, u64le, usage_error, write};

const LC_SEGMENT_64: u32 = 0x19;
const LC_UNIXTHREAD: u32 = 0x5;
const LC_DYLD_CHAINED_FIXUPS: u32 = 0x80000034;
const PAGE: u64 = 0x4000;
const BOOT_LINE_LENGTH: usize = 608;
const CMDLINE: usize = 0x48;
const SEG_TABLE: usize = CMDLINE + BOOT_LINE_LENGTH;

pub struct Seg {
    pub name: String,
    pub vmaddr: u64,
    pub vmsize: u64,
    pub fileoff: u64,
    pub filesize: u64,
}

pub struct Macho {
    pub cputype: u32,
    pub cpusub: u32,
    pub segs: Vec<Seg>,
    pub entry: Option<u64>,
    pub fixups: Option<(u32, u32)>,
}

pub fn parse_macho(buf: &[u8]) -> Macho {
    if u32le(buf, 0) != 0xfeedfacf {
        die("not a 64-bit Mach-O");
    }
    let mut m = Macho { cputype: u32le(buf, 4), cpusub: u32le(buf, 8) & 0xff, segs: vec![], entry: None, fixups: None };
    let ncmds = u32le(buf, 16);
    let mut off = 32;
    for _ in 0..ncmds {
        let cmd = u32le(buf, off);
        let size = u32le(buf, off + 4) as usize;
        if cmd == LC_SEGMENT_64 {
            m.segs.push(Seg {
                name: cstr_at(&buf[off + 8..off + 24]),
                vmaddr: u64le(buf, off + 24),
                vmsize: u64le(buf, off + 32),
                fileoff: u64le(buf, off + 40),
                filesize: u64le(buf, off + 48),
            });
        } else if cmd == LC_UNIXTHREAD {
            m.entry = Some(u64le(buf, off + 16 + 32 * 8));
        } else if cmd == LC_DYLD_CHAINED_FIXUPS {
            m.fixups = Some((u32le(buf, off + 8), u32le(buf, off + 12)));
        }
        off += size;
    }
    m
}

fn apply_chained_fixups(buf: &mut [u8], m: &Macho) -> u64 {
    let Some((data_off, _)) = m.fixups else { return 0 };
    let data_off = data_off as usize;
    let starts_off = u32le(buf, data_off + 4) as usize;
    if u32le(buf, data_off + 16) != 0 {
        die("kernel has binds; only rebases are supported");
    }
    let image_base = m.segs[0].vmaddr;
    let seg_count = u32le(buf, data_off + starts_off) as usize;
    let mut nfix = 0;
    for i in 0..seg_count {
        let seg_info = u32le(buf, data_off + starts_off + 4 + 4 * i) as usize;
        if seg_info == 0 {
            continue;
        }
        let base = data_off + starts_off + seg_info;
        let page_size = u16le(buf, base + 4) as u64;
        let fmt = u16le(buf, base + 6);
        let page_count = u16le(buf, base + 20) as usize;
        let seg = &m.segs[i];
        for pg in 0..page_count {
            let start = u16le(buf, base + 22 + 2 * pg);
            if start == 0xffff {
                continue;
            }
            if start & 0x8000 != 0 {
                die("DYLD_CHAINED_PTR_START_MULTI not supported");
            }
            let mut loc = (seg.fileoff + pg as u64 * page_size + start as u64) as usize;
            loop {
                let raw = u64le(buf, loc);
                let (target, nxt, stride) = decode(raw, fmt, image_base);
                put_u64(buf, loc, target);
                nfix += 1;
                if nxt == 0 {
                    break;
                }
                loc += (nxt * stride) as usize;
            }
        }
    }
    nfix
}

fn decode(raw: u64, fmt: u16, image_base: u64) -> (u64, u64, u64) {
    match fmt {
        2 | 6 => {
            if raw >> 63 != 0 {
                die("bind in kernel");
            }
            let mut target = raw & ((1 << 36) - 1);
            let high8 = (raw >> 36) & 0xff;
            let nxt = (raw >> 51) & 0xfff;
            if fmt == 6 {
                target = target.wrapping_add(image_base);
            }
            (target | high8 << 56, nxt, 4)
        }
        8 => (image_base.wrapping_add(raw & ((1 << 30) - 1)), (raw >> 51) & 0xfff, 4),
        1 | 7 | 9 | 12 => {
            let auth = raw >> 63;
            let nxt = (raw >> 51) & 0x7ff;
            let stride = if fmt == 7 || fmt == 12 { 4 } else { 8 };
            let mut target = if auth != 0 {
                raw & 0xffffffff
            } else {
                (raw & ((1 << 43) - 1)) | ((raw >> 43) & 0xff) << 56
            };
            if fmt != 1 || auth != 0 {
                target = target.wrapping_add(image_base);
            }
            (target, nxt, stride)
        }
        _ => die(format!("unsupported chained pointer format {fmt}")),
    }
}

fn add_ramdisk_to_adt(blob: &[u8], phys: u64, size: u64) -> Vec<u8> {
    let (mut root, _) = adt::parse(blob, 0);
    let mmap = root
        .child_mut("chosen")
        .and_then(|c| c.child_mut("memory-map"))
        .unwrap_or_else(|| die("ADT has no /chosen/memory-map"));
    if mmap.prop("RAMDisk").is_some() {
        die("ADT already has a RAMDisk");
    }
    let mut v = phys.to_le_bytes().to_vec();
    v.extend(size.to_le_bytes());
    mmap.props.push(("RAMDisk".into(), 0, v));
    adt::serialize(&root)
}

fn adt_dram(blob: &[u8]) -> (u64, u64) {
    let (root, _) = adt::parse(blob, 0);
    let chosen = root.child("chosen").unwrap_or_else(|| die("ADT has no /chosen"));
    let get = |k: &str| u64le(chosen.prop(k).unwrap_or_else(|| die(format!("ADT has no /chosen/{k}"))), 0);
    (get("dram-base"), get("dram-size"))
}

fn round_page(v: u64) -> u64 {
    (v + PAGE - 1) & !(PAGE - 1)
}

pub fn main(args: &[String]) -> i32 {
    let o = parse("mkpayload", args, &["--cmdline", "--ramdisk"], &[]);
    if o.pos.len() != 3 {
        usage_error("mkpayload", "usage: sweet mkpayload KERNEL ADT OUT [--cmdline \"...\"] [--ramdisk IMAGE]");
    }
    let cmdline = o.get("--cmdline").unwrap_or("debug=0xc serial=3 cpus=1 keepsyms=1");

    let mut buf = read(&o.pos[0]);
    let m = parse_macho(&buf);
    let nfix = apply_chained_fixups(&mut buf, &m);
    let segs: Vec<&Seg> = m.segs.iter().filter(|s| s.vmsize != 0).collect();
    let text = &m.segs[0];
    if text.name != "__TEXT" || text.fileoff != 0 {
        die("first segment is not __TEXT at file offset 0");
    }
    let entry = m.entry.unwrap_or_else(|| die("no LC_UNIXTHREAD"));
    let virt_base = text.vmaddr & !(0x2000000 - 1);
    let mut kernel_end = round_page(segs.iter().map(|s| s.vmaddr + s.vmsize).max().unwrap());

    let mut adt_blob = read(&o.pos[1]);
    let (phys_base, mem_size) = adt_dram(&adt_blob);
    let mut pieces: Vec<(u64, u64, Vec<u8>)> = segs
        .iter()
        .map(|s| (s.vmaddr, s.vmsize, buf[s.fileoff as usize..(s.fileoff + s.filesize) as usize].to_vec()))
        .collect();
    let mut ramdisk = "None".to_string();
    if let Some(rd) = o.get("--ramdisk") {
        let image = read(rd);
        let rd_size = round_page(image.len() as u64);
        let rd_va = kernel_end;
        kernel_end = rd_va + rd_size;
        let rd_pa = rd_va - virt_base + phys_base;
        pieces.push((rd_va, rd_size, image));
        adt_blob = add_ramdisk_to_adt(&adt_blob, rd_pa, rd_size);
        ramdisk = format!("('{rd_pa:#x}', '{rd_size:#x}')");
    }

    let mut out = vec![0u8; SEG_TABLE + 32 * pieces.len()];
    let mut seg_table = vec![];
    for (vmaddr, vmsize, data) in &pieces {
        let off = out.len();
        out.extend(data);
        out.resize(out.len() + (out.len().wrapping_neg() & 0xf), 0);
        seg_table.push([*vmaddr, *vmsize, off as u64, data.len() as u64]);
    }
    let adt_off = out.len();
    out.extend(&adt_blob);
    out.resize(out.len() + (out.len().wrapping_neg() & 0xf), 0);

    out[..8].copy_from_slice(b"SWEETXNU");
    put_u32(&mut out, 8, 1);
    put_u32(&mut out, 12, pieces.len() as u32);
    for (i, v) in [virt_base, entry, adt_off as u64, adt_blob.len() as u64, kernel_end, phys_base, mem_size]
        .iter()
        .enumerate()
    {
        put_u64(&mut out, 0x10 + 8 * i, *v);
    }
    let cl = &cmdline.as_bytes()[..cmdline.len().min(BOOT_LINE_LENGTH - 1)];
    out[CMDLINE..CMDLINE + cl.len()].copy_from_slice(cl);
    for (i, t) in seg_table.iter().enumerate() {
        for (j, v) in t.iter().enumerate() {
            put_u64(&mut out, SEG_TABLE + 32 * i + 8 * j, *v);
        }
    }
    write(&o.pos[2], &out);
    println!("cputype: {:#x}", m.cputype);
    println!("cpusub: {}", m.cpusub);
    println!("fixups: {nfix}");
    println!("virt_base: {virt_base:#x}");
    println!("entry: {entry:#x}");
    println!("kernel_end: {kernel_end:#x}");
    println!("ramdisk: {ramdisk}");
    println!("size: {}", out.len());
    for s in &segs {
        println!("   {} {:#x} {:#x}", s.name, s.vmaddr, s.vmsize);
    }
    0
}
