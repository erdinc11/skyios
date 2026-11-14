use crate::util::{die, u16le, u32le, u64le};
use flate2::{Decompress, FlushDecompress, Status};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

const CHUNK: u64 = 64 << 20;

fn curl(args: &[&str]) -> Vec<u8> {
    let out = Command::new("curl")
        .args(["-sfL", "--max-time", "300"])
        .args(args)
        .output()
        .unwrap_or_else(|e| die(format!("curl: {e}")));
    if !out.status.success() {
        die(format!("curl {} failed: {}", args.join(" "), out.status));
    }
    out.stdout
}

fn get(url: &str, start: u64, end: u64) -> Vec<u8> {
    let data = curl(&["-r", &format!("{start}-{end}"), url]);
    if data.len() as u64 != end - start + 1 {
        die(format!("bad range reply ({} bytes)", data.len()));
    }
    data
}

fn size_of(url: &str) -> u64 {
    for line in String::from_utf8_lossy(&curl(&["-I", url])).lines() {
        if line.to_lowercase().starts_with("content-length:") {
            if let Ok(n) = line.split(':').nth(1).unwrap().trim().parse() {
                return n;
            }
        }
    }
    die("no Content-Length")
}

fn rfind(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).rposition(|w| w == n)
}

struct Entry {
    name: String,
    method: u16,
    csize: u64,
    usize: u64,
    lho: u64,
    crc: u32,
}

fn central_dir(url: &str) -> Vec<Entry> {
    let total = size_of(url);
    let tail = get(url, total.saturating_sub(65536 + 22), total - 1);
    let eocd = rfind(&tail, b"PK\x05\x06").unwrap_or_else(|| die("no EOCD"));
    let (mut cd_size, mut cd_off) = (u32le(&tail, eocd + 12) as u64, u32le(&tail, eocd + 16) as u64);
    if let Some(loc) = rfind(&tail, b"PK\x06\x06") {
        cd_size = u64le(&tail, loc + 40);
        cd_off = u64le(&tail, loc + 48);
    }
    let cd = get(url, cd_off, cd_off + cd_size - 1);
    let mut i = 0;
    let mut out = vec![];
    while i + 4 <= cd.len() && &cd[i..i + 4] == b"PK\x01\x02" {
        let method = u16le(&cd, i + 10);
        let crc = u32le(&cd, i + 16);
        let mut csize = u32le(&cd, i + 20) as u64;
        let mut usize = u32le(&cd, i + 24) as u64;
        let (nlen, xlen, clen) = (u16le(&cd, i + 28) as usize, u16le(&cd, i + 30) as usize, u16le(&cd, i + 32) as usize);
        let mut lho = u32le(&cd, i + 42) as u64;
        let name = String::from_utf8_lossy(&cd[i + 46..i + 46 + nlen]).into_owned();
        let extra = &cd[i + 46 + nlen..i + 46 + nlen + xlen];
        let mut j = 0;
        while j < extra.len() {
            let (hid, hlen) = (u16le(extra, j), u16le(extra, j + 2) as usize);
            if hid == 1 {
                let mut vals = (0..hlen / 8).map(|k| u64le(extra, j + 4 + 8 * k));
                if usize == 0xffffffff {
                    usize = vals.next().unwrap();
                }
                if csize == 0xffffffff {
                    csize = vals.next().unwrap();
                }
                if lho == 0xffffffff {
                    lho = vals.next().unwrap();
                }
            }
            j += 4 + hlen;
        }
        out.push(Entry { name, method, csize, usize, lho, crc });
        i += 46 + nlen + xlen + clen;
    }
    out
}

fn inflate(z: &mut Decompress, mut input: &[u8], out: &mut Vec<u8>, finish: bool) {
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let (i0, o0) = (z.total_in(), z.total_out());
        let flush = if finish { FlushDecompress::Finish } else { FlushDecompress::None };
        let st = z.decompress(input, &mut buf, flush).unwrap_or_else(|e| die(format!("inflate: {e}")));
        let used = (z.total_in() - i0) as usize;
        let made = (z.total_out() - o0) as usize;
        out.extend(&buf[..made]);
        input = &input[used..];
        if st == Status::StreamEnd || (input.is_empty() && made < buf.len()) || (used == 0 && made == 0) {
            break;
        }
    }
}

fn extract(url: &str, e: &Entry, outdir: &str) {
    let path = Path::new(outdir).join(&e.name);
    if e.name.ends_with('/') {
        fs::create_dir_all(&path).unwrap_or_else(|err| die(err));
        return;
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent).unwrap_or_else(|err| die(err));
    if fs::metadata(&path).map(|m| m.len() == e.usize).unwrap_or(false) {
        println!("{}: already complete", e.name);
        return;
    }
    let hdr = get(url, e.lho, e.lho + 29);
    let start = e.lho + 30 + u16le(&hdr, 26) as u64 + u16le(&hdr, 28) as u64;
    let part = format!("{}.part", path.display());
    let mut done = if e.method == 0 { fs::metadata(&part).map(|m| m.len()).unwrap_or(0) } else { 0 };
    let mut z = (e.method == 8).then(|| Decompress::new(false));
    let mut out = if done > 0 {
        OpenOptions::new().append(true).open(&part)
    } else {
        File::create(&part)
    }
    .unwrap_or_else(|err| die(format!("{part}: {err}")));
    while done < e.csize {
        let n = CHUNK.min(e.csize - done);
        let data = get(url, start + done, start + done + n - 1);
        match z.as_mut() {
            Some(z) => {
                let mut o = vec![];
                inflate(z, &data, &mut o, false);
                out.write_all(&o)
            }
            None => out.write_all(&data),
        }
        .unwrap_or_else(|err| die(err));
        done += n;
        println!("{}: {}% ({} MiB)", e.name, done * 100 / e.csize, done >> 20);
    }
    if let Some(z) = z.as_mut() {
        let mut o = vec![];
        inflate(z, &[], &mut o, true);
        out.write_all(&o).unwrap_or_else(|err| die(err));
    }
    drop(out);
    let mut f = File::open(&part).unwrap_or_else(|err| die(err));
    let mut h = crc32fast::Hasher::new();
    let mut buf = vec![0u8; CHUNK as usize];
    loop {
        let n = f.read(&mut buf).unwrap_or_else(|err| die(err));
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let got = h.finalize();
    let len = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if len != e.usize {
        die(format!("{}: {len} != {} bytes", e.name, e.usize));
    }
    if got != e.crc {
        die(format!("{}: CRC-32 {got:08x} != {:08x}", e.name, e.crc));
    }
    fs::rename(&part, &path).unwrap_or_else(|err| die(err));
    println!("{}: {} bytes, CRC-32 ok", e.name, e.usize);
}

pub fn main(args: &[String]) -> i32 {
    let Some(url) = args.first() else {
        eprintln!("usage: skyios remotezip URL [PATTERN OUTDIR]");
        return 2;
    };
    let entries = central_dir(url);
    if args.len() == 1 {
        for e in &entries {
            println!("{:>12} {}", e.usize, e.name);
        }
    } else if args.len() >= 3 {
        for e in entries.iter().filter(|e| e.name.contains(args[1].as_str())) {
            extract(url, e, &args[2]);
        }
    } else {
        eprintln!("usage: skyios remotezip URL [PATTERN OUTDIR]");
        return 2;
    }
    0
}
