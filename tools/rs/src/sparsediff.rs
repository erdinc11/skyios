use crate::util::{die, parse, usage_error};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};

const MAGIC: u32 = 0xed26ff3a;
const RAW: u16 = 0xcac1;
const DONT_CARE: u16 = 0xcac3;

fn size_of(p: &str) -> u64 {
    std::fs::metadata(p).unwrap_or_else(|e| die(format!("{p}: {e}"))).len()
}

fn open(p: &str) -> File {
    File::open(p).unwrap_or_else(|e| die(format!("{p}: {e}")))
}

fn read_full(f: &mut File, buf: &mut [u8]) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) => die(e),
        }
    }
    n
}

pub fn main(args: &[String]) -> i32 {
    let o = parse("sparsediff", args, &["--block"], &[]);
    if o.pos.len() != 3 {
        usage_error("sparsediff", "usage: sweet sparsediff OLD.img NEW.img OUT.simg [--block 4096]");
    }
    let bs: u64 = o
        .get("--block")
        .map(|b| b.parse().unwrap_or_else(|_| usage_error("sparsediff", &format!("argument --block: invalid int value: '{b}'"))))
        .unwrap_or(4096);
    let (old, new, outp) = (&o.pos[0], &o.pos[1], &o.pos[2]);
    let size = size_of(new);
    if size_of(old) != size || !size.is_multiple_of(bs) {
        die("images must have the same size, a multiple of the block size");
    }
    let nblocks = size / bs;

    let mut runs: Vec<(bool, u64)> = vec![];
    let step = (8u64 << 20) / bs;
    let (mut fo, mut fnew) = (open(old), open(new));
    let mut bo = vec![0u8; (step * bs) as usize];
    let mut bn = vec![0u8; (step * bs) as usize];
    let mut base = 0;
    while base < nblocks {
        let n = step.min(nblocks - base);
        let len = (n * bs) as usize;
        read_full(&mut fo, &mut bo[..len]);
        read_full(&mut fnew, &mut bn[..len]);
        for i in 0..n as usize {
            let r = i * bs as usize..(i + 1) * bs as usize;
            let d = bo[r.clone()] != bn[r];
            match runs.last_mut() {
                Some(last) if last.0 == d => last.1 += 1,
                _ => runs.push((d, 1)),
            }
        }
        base += step;
    }

    let changed: u64 = runs.iter().filter(|r| r.0).map(|r| r.1).sum();
    let mut fnew = open(new);
    let mut out = BufWriter::new(File::create(outp).unwrap_or_else(|e| die(format!("{outp}: {e}"))));
    let mut hdr = vec![];
    hdr.extend(MAGIC.to_le_bytes());
    for v in [1u16, 0, 28, 12] {
        hdr.extend(v.to_le_bytes());
    }
    for v in [bs as u32, nblocks as u32, runs.len() as u32, 0] {
        hdr.extend(v.to_le_bytes());
    }
    let w = |out: &mut BufWriter<File>, b: &[u8]| out.write_all(b).unwrap_or_else(|e| die(e));
    w(&mut out, &hdr);
    let mut pos = 0;
    let mut chunk = vec![0u8; 8 << 20];
    for &(d, count) in &runs {
        let mut c = vec![];
        c.extend((if d { RAW } else { DONT_CARE }).to_le_bytes());
        c.extend(0u16.to_le_bytes());
        c.extend((count as u32).to_le_bytes());
        c.extend((if d { 12 + count * bs } else { 12 } as u32).to_le_bytes());
        w(&mut out, &c);
        if d {
            fnew.seek(SeekFrom::Start(pos * bs)).unwrap_or_else(|e| die(e));
            let mut left = (count * bs) as usize;
            while left > 0 {
                let n = read_full(&mut fnew, &mut chunk[..left.min(8 << 20)]);
                if n == 0 {
                    die("short read");
                }
                w(&mut out, &chunk[..n]);
                left -= n;
            }
        }
        pos += count;
    }
    out.flush().unwrap_or_else(|e| die(e));
    println!(
        "{outp}: {changed} of {nblocks} blocks changed ({} MiB) in {} runs",
        (changed * bs) >> 20,
        runs.iter().filter(|r| r.0).count()
    );
    0
}
