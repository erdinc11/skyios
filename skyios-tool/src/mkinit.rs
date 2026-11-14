use crate::util::{cstr_at, die, read, u32le, u64le, write};
use sha2::{Digest, Sha256};

const MH_MAGIC_64: u32 = 0xfeedfacf;
const CPU_TYPE_ARM64: u32 = 0x0100000c;
const MH_EXECUTE: u32 = 2;
const MH_NOUNDEFS: u32 = 1;
const LC_SEGMENT_64: u32 = 0x19;
const LC_SYMTAB: u32 = 0x2;
const LC_UNIXTHREAD: u32 = 0x5;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const ARM_THREAD_STATE64: u32 = 6;
const ARM_THREAD_STATE64_COUNT: u32 = 68;
const VM_PROT_R: u32 = 1;
const VM_PROT_X: u32 = 4;
const S_ATTR_CODE: u32 = 0x80000400;
const BASE: u64 = 0x100000000;
const SEG: usize = 0x4000;
const CODE_OFF: usize = 0x400;
const CS_PAGE_SHIFT: u8 = 12;
const IDENT: &[u8] = b"org.sweet.init\0";

fn text_of(obj: &[u8]) -> (Vec<u8>, u64) {
    if u32le(obj, 0) != MH_MAGIC_64 || u32le(obj, 4) != CPU_TYPE_ARM64 || u32le(obj, 12) != 1 {
        die("not an arm64 object");
    }
    let ncmds = u32le(obj, 16);
    let (mut text, mut symtab, mut off) = (None, None, 32);
    for _ in 0..ncmds {
        let cmd = u32le(obj, off);
        let size = u32le(obj, off + 4) as usize;
        if cmd == LC_SEGMENT_64 {
            let nsects = u32le(obj, off + 64) as usize;
            for i in 0..nsects {
                let s = off + 72 + 80 * i;
                let sect = cstr_at(&obj[s..s + 16]);
                let seg = cstr_at(&obj[s + 16..s + 32]);
                let ssize = u64le(obj, s + 40) as usize;
                let soff = u32le(obj, s + 48) as usize;
                let nreloc = u32le(obj, s + 60);
                if nreloc != 0 {
                    die(format!("{seg},{sect} has relocations: code must be position independent"));
                }
                if seg == "__TEXT" && sect == "__text" {
                    text = Some(obj[soff..soff + ssize].to_vec());
                } else if ssize != 0 {
                    die(format!("unexpected non-empty section {seg},{sect}"));
                }
            }
        } else if cmd == LC_SYMTAB {
            symtab = Some((u32le(obj, off + 8) as usize, u32le(obj, off + 12), u32le(obj, off + 16) as usize));
        }
        off += size;
    }
    let (Some(text), Some((symoff, nsyms, stroff))) = (text, symtab) else { die("no __text or symbol table") };
    for i in 0..nsyms as usize {
        let e = symoff + 16 * i;
        let strx = u32le(obj, e) as usize;
        if cstr_at(&obj[stroff + strx..]) == "_start" && obj[e + 4] & 1 != 0 {
            return (text, u64le(obj, e + 8));
        }
    }
    die("no global _start")
}

fn be(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn le32(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn name16(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.resize(16, 0);
    v
}

fn segment(name: &str, vmaddr: u64, vmsize: u64, fileoff: u64, filesize: u64, prot: u32, sections: &[Vec<u8>]) -> Vec<u8> {
    let mut c = le32(&[LC_SEGMENT_64, 72 + 80 * sections.len() as u32]);
    c.extend(name16(name));
    for v in [vmaddr, vmsize, fileoff, filesize] {
        c.extend(v.to_le_bytes());
    }
    c.extend(le32(&[prot, prot, sections.len() as u32, 0]));
    for s in sections {
        c.extend(s);
    }
    c
}

fn code_signature(image: &[u8], code_limit: usize, text_size: u64, entitlements: Option<&[u8]>) -> Vec<u8> {
    let page = 1 << CS_PAGE_SHIFT;
    let mut hashes: Vec<u8> = (0..code_limit)
        .step_by(page)
        .flat_map(|o| Sha256::digest(&image[o..(o + page).min(code_limit)]).to_vec())
        .collect();
    let nslots = (hashes.len() / 32) as u32;
    let mut ent_blob = None;
    let mut nspecial = 0u32;
    if let Some(ents) = entitlements {
        let mut b = be(&[0xfade7171, 8 + ents.len() as u32]);
        b.extend(ents);
        let mut h = Sha256::digest(&b).to_vec();
        h.extend([0u8; 32 * 4]);
        h.extend(&hashes);
        hashes = h;
        ent_blob = Some(b);
        nspecial = 5;
    }
    let hdr_len = 88u32;
    let ident_off = hdr_len;
    let hash_off = ident_off + IDENT.len() as u32 + 32 * nspecial;
    let cd_len = ident_off + IDENT.len() as u32 + hashes.len() as u32;
    let mut cd = be(&[0xfade0c02, cd_len, 0x20400, 0x2, hash_off, ident_off, nspecial, nslots, code_limit as u32]);
    cd.extend([32, 2, 0, CS_PAGE_SHIFT]);
    cd.extend(be(&[0, 0, 0, 0]));
    for v in [0u64, 0, text_size, 1] {
        cd.extend(v.to_be_bytes());
    }
    assert_eq!(cd.len(), hdr_len as usize);
    cd.extend(IDENT);
    cd.extend(&hashes);
    match ent_blob {
        None => {
            let sb_hdr = 12 + 8;
            let mut out = be(&[0xfade0cc0, sb_hdr + cd.len() as u32, 1, 0, sb_hdr]);
            out.extend(cd);
            out
        }
        Some(eb) => {
            let sb_hdr = 12 + 8 * 2;
            let mut out = be(&[0xfade0cc0, sb_hdr + (cd.len() + eb.len()) as u32, 2, 0, sb_hdr, 5, sb_hdr + cd.len() as u32]);
            out.extend(cd);
            out.extend(eb);
            out
        }
    }
}

pub fn build(obj: &[u8], entitlements: Option<&[u8]>) -> Vec<u8> {
    let (code, start) = text_of(obj);
    if CODE_OFF + code.len() > SEG {
        die("code does not fit in one __TEXT page");
    }
    let mut sect = name16("__text");
    sect.extend(name16("__TEXT"));
    sect.extend((BASE + CODE_OFF as u64).to_le_bytes());
    sect.extend((code.len() as u64).to_le_bytes());
    sect.extend(le32(&[CODE_OFF as u32, 3, 0, 0, S_ATTR_CODE, 0, 0, 0]));
    let sig_size = code_signature(&vec![0; SEG], SEG, SEG as u64, entitlements).len();
    let mut thread = le32(&[LC_UNIXTHREAD, 16 + 4 * ARM_THREAD_STATE64_COUNT, ARM_THREAD_STATE64, ARM_THREAD_STATE64_COUNT]);
    let mut state = [0u64; 33];
    state[32] = BASE + CODE_OFF as u64 + start;
    for v in state {
        thread.extend(v.to_le_bytes());
    }
    thread.extend(le32(&[0, 0]));
    let cmds = [
        segment("__PAGEZERO", 0, BASE, 0, 0, 0, &[]),
        segment("__TEXT", BASE, SEG as u64, 0, SEG as u64, VM_PROT_R | VM_PROT_X, &[sect]),
        segment("__LINKEDIT", BASE + SEG as u64, SEG as u64, SEG as u64, sig_size as u64, VM_PROT_R, &[]),
        thread,
        le32(&[LC_CODE_SIGNATURE, 16, SEG as u32, sig_size as u32]),
    ];
    let lc: Vec<u8> = cmds.concat();
    let header = le32(&[MH_MAGIC_64, CPU_TYPE_ARM64, 0, MH_EXECUTE, cmds.len() as u32, lc.len() as u32, MH_NOUNDEFS, 0]);
    assert!(32 + lc.len() <= CODE_OFF);
    let mut image = vec![0u8; SEG];
    image[..32].copy_from_slice(&header);
    image[32..32 + lc.len()].copy_from_slice(&lc);
    image[CODE_OFF..CODE_OFF + code.len()].copy_from_slice(&code);
    let sig = code_signature(&image, SEG, SEG as u64, entitlements);
    assert_eq!(sig.len(), sig_size);
    image.extend(sig);
    image
}

pub fn main(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: skyios mkinit OBJECT OUT [ENTITLEMENTS.plist]");
        return 2;
    }
    let ents = args.get(2).map(read);
    let out = build(&read(&args[0]), ents.as_deref());
    write(&args[1], &out);
    println!("{}: {} bytes", args[1], out.len());
    0
}
