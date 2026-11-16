use crate::logpull::{adb, adb_text};
use crate::util::{die, u32le, write};
use flate2::{write::ZlibEncoder, Compression};
use std::io::Write;

// Screenshots written by the kernel's sweetshot (xnu bsd/dev/sweet_ufs.c):
// SLOTS slots of SLOT_BLOCKS blocks just before the 1025-block log ring at
// the end of userdata; each slot = header block ("SWEETSHT", version, width,
// height, bytes per row, slot, seconds) + BGRA rows.
const BLOCK: u64 = 4096;
const LOG_BLOCKS: u64 = 1025;
const SLOTS: u64 = 4;
const SLOT_BLOCKS: u64 = 1024;

fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut c = crc32fast::Hasher::new();
    c.update(kind);
    c.update(data);
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    png.extend_from_slice(&c.finalize().to_be_bytes());
}

// 8-bit RGB PNG from BGRA rows
fn png(bgra: &[u8], w: usize, h: usize, bpr: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(h * (1 + 3 * w));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let p = &bgra[y * bpr + 4 * x..][..4];
            raw.extend_from_slice(&[p[2], p[1], p[0]]);
        }
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
    z.write_all(&raw).unwrap();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z.finish().unwrap());
    chunk(&mut out, b"IEND", &[]);
    out
}

pub fn main(args: &[String]) -> i32 {
    let (serial, rest) = match args {
        [flag, s, rest @ ..] if flag == "-s" => (Some(s.as_str()), rest),
        _ => (None, args),
    };
    let Some(prefix) = rest.first() else {
        eprintln!("usage: skyios shot-pull [-s SERIAL] OUT_PREFIX   (sweetshot screenshots from userdata via recovery adb -> OUT_PREFIX-N.png)");
        return 2;
    };
    let dev = adb_text(
        serial,
        &["shell", "for d in /dev/block/by-name/userdata /dev/block/bootdevice/by-name/userdata; do [ -e $d ] && readlink -f $d && break; done"],
    );
    if dev.is_empty() {
        die("userdata block device not found on the connected device");
    }
    let bytes: u64 = adb_text(serial, &["shell", "blockdev", "--getsize64", &dev])
        .parse()
        .unwrap_or_else(|e| die(format!("blockdev size: {e}")));
    let skip = bytes / BLOCK - LOG_BLOCKS - SLOTS * SLOT_BLOCKS;
    let area = adb(serial, &["exec-out", &format!("dd if={dev} bs=4096 skip={skip} count={} 2>/dev/null", SLOTS * SLOT_BLOCKS)]);
    let mut found = 0;
    for slot in 0..SLOTS as usize {
        let base = slot * (SLOT_BLOCKS * BLOCK) as usize;
        if area.len() < base + BLOCK as usize || &area[base..base + 8] != b"SWEETSHT" {
            continue;
        }
        let (w, h, bpr, secs) = (
            u32le(&area, base + 12) as usize,
            u32le(&area, base + 16) as usize,
            u32le(&area, base + 20) as usize,
            u32le(&area, base + 28),
        );
        let data = base + BLOCK as usize;
        if w == 0 || h == 0 || bpr < 4 * w || area.len() < data + bpr * h {
            eprintln!("slot {slot}: bad header {w}x{h} bpr {bpr}");
            continue;
        }
        let out = format!("{prefix}-{slot}.png");
        write(&out, &png(&area[data..], w, h, bpr));
        println!("{out}: {w}x{h}, taken {secs} s after the log started");
        found += 1;
    }
    if found == 0 {
        die("no SWEETSHT screenshot in the reserved area (boot with sweetshot=S)");
    }
    0
}
