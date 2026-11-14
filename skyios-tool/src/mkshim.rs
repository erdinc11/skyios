use crate::util::{die, parse, read, u16le, u32le, u64le, usage_error, write};
use sha1::{Digest, Sha1};

const USAGE: &str = "usage: skyios mkshim SHIM PAYLOAD DTB OUT.img [--cmdline STR] [--image OUT.Image]
  SHIM is the shim ELF (target/aarch64-unknown-none-softfloat/release/skyios-shim) or a flat Image.
  Packs an Android boot.img v2 like mkbootimg --header_version 2 --base 0 --kernel_offset 0x8000
  --ramdisk_offset 0x1000000 --tags_offset 0x100 --dtb_offset 0x1f00000 --pagesize 4096
  --os_version 17.0.0 --os_patch_level 2035-06.";

const PAGE: usize = 4096;
const KERNEL_ADDR: u32 = 0x8000;
const RAMDISK_ADDR: u32 = 0x0100_0000;
const SECOND_ADDR: u32 = 0;
const TAGS_ADDR: u32 = 0x100;
const DTB_ADDR: u64 = 0x01f0_0000;
const OS_VERSION: u32 = (17 << 25) | ((2035 - 2000) << 4) | 6;
const HEADER_V2_SIZE: u32 = 1660;
const R_AARCH64_RELATIVE: u64 = 1027;

fn flat_image(elf: &[u8]) -> Vec<u8> {
    if elf.len() < 64 || elf[4] != 2 || elf[5] != 1 || u16le(elf, 18) != 183 {
        die("shim ELF must be ELF64 little-endian aarch64");
    }
    let (phoff, phentsize, phnum) = (u64le(elf, 32) as usize, u16le(elf, 54) as usize, u16le(elf, 56) as usize);
    let (shoff, shentsize, shnum) = (u64le(elf, 40) as usize, u16le(elf, 58) as usize, u16le(elf, 60) as usize);
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        if u32le(elf, sh + 4) == 4 {
            let (off, size) = (u64le(elf, sh + 24) as usize, u64le(elf, sh + 32) as usize);
            for r in (off..off + size).step_by(24) {
                let kind = u64le(elf, r + 8) & 0xffff_ffff;
                if kind != R_AARCH64_RELATIVE {
                    die(format!("shim ELF has relocation type {kind}; only R_AARCH64_RELATIVE is applied at boot"));
                }
            }
        }
    }
    let mut segs = Vec::new();
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if u32le(elf, ph) == 1 {
            segs.push((u64le(elf, ph + 8) as usize, u64le(elf, ph + 16) as usize, u64le(elf, ph + 32) as usize));
        }
    }
    let Some(&(head_off, _, _)) = segs.iter().find(|s| s.1 == 0) else {
        die("no PT_LOAD at address 0");
    };
    if &elf[head_off + 56..head_off + 60] != b"ARM\x64" {
        die("shim does not start with an arm64 Image header");
    }
    let size = u64le(elf, head_off + 16) as usize;
    let mut img = vec![0u8; size];
    for (off, va, filesz) in segs {
        if va + filesz > size {
            die(format!("segment at {va:#x} beyond the image end {size:#x}"));
        }
        img[va..va + filesz].copy_from_slice(&elf[off..off + filesz]);
    }
    img
}

fn put32(h: &mut [u8], o: usize, v: u32) {
    h[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn pad(out: &mut Vec<u8>) {
    out.resize(out.len().div_ceil(PAGE) * PAGE, 0);
}

pub fn main(args: &[String]) -> i32 {
    let o = parse("mkshim", args, &["--cmdline", "--image"], &[]);
    if o.pos.len() != 4 {
        usage_error("mkshim", USAGE);
    }
    let shim = read(&o.pos[0]);
    let kernel = if shim.starts_with(b"\x7fELF") { flat_image(&shim) } else { shim };
    if let Some(p) = o.get("--image") {
        write(p, &kernel);
    }
    let (ramdisk, dtb) = (read(&o.pos[1]), read(&o.pos[2]));
    let cmdline = o.get("--cmdline").unwrap_or("console=null");
    if cmdline.len() >= 512 {
        die("cmdline longer than 511 bytes");
    }

    let mut sha = Sha1::new();
    for part in [&kernel[..], &ramdisk[..], &[][..], &[][..], &dtb[..]] {
        sha.update(part);
        sha.update((part.len() as u32).to_le_bytes());
    }
    let id = sha.finalize();

    let mut h = vec![0u8; HEADER_V2_SIZE as usize];
    h[..8].copy_from_slice(b"ANDROID!");
    for (o, v) in [
        (8, kernel.len() as u32),
        (12, KERNEL_ADDR),
        (16, ramdisk.len() as u32),
        (20, RAMDISK_ADDR),
        (24, 0),
        (28, SECOND_ADDR),
        (32, TAGS_ADDR),
        (36, PAGE as u32),
        (40, 2),
        (44, OS_VERSION),
        (1644, HEADER_V2_SIZE),
        (1648, dtb.len() as u32),
    ] {
        put32(&mut h, o, v);
    }
    h[64..64 + cmdline.len()].copy_from_slice(cmdline.as_bytes());
    h[576..576 + id.len()].copy_from_slice(&id);
    h[1652..1660].copy_from_slice(&DTB_ADDR.to_le_bytes());

    let mut out = h;
    pad(&mut out);
    for part in [&kernel, &ramdisk, &dtb] {
        out.extend_from_slice(part);
        pad(&mut out);
    }
    write(&o.pos[3], &out);
    eprintln!("{}: kernel {} bytes, ramdisk {} bytes, dtb {} bytes", o.pos[3], kernel.len(), ramdisk.len(), dtb.len());
    0
}
