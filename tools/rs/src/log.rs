use crate::util::{die, read, u32le, u64le, write};
use std::io::Write;

pub fn main(args: &[String]) -> i32 {
    let Some(src) = args.first() else {
        eprintln!("usage: sweet log AREA.bin [OUT.txt]");
        return 2;
    };
    let data = read(src);
    if data.len() < 32 || &data[..8] != b"SWEETLOG" {
        die("no SWEETLOG header (log never flushed, or wrong area)");
    }
    let (version, size, total, seq) = (u32le(&data, 8), u32le(&data, 12) as u64, u64le(&data, 16), u64le(&data, 24));
    let ring = &data[4096.min(data.len())..(4096 + size as usize).min(data.len())];
    let text: Vec<u8> = if total <= size {
        ring[..(total as usize).min(ring.len())].to_vec()
    } else {
        let start = ((total % size) as usize).min(ring.len());
        [&ring[start..], &ring[..start]].concat()
    };
    let out = String::from_utf8_lossy(&text).replace('\r', "");
    eprintln!(
        "# version {version}, ring {size}, total {total} bytes, flush #{seq}{}",
        if total > size { " (wrapped: oldest bytes lost)" } else { "" }
    );
    match args.get(1) {
        Some(p) => write(p, out.as_bytes()),
        None => std::io::stdout().write_all(out.as_bytes()).unwrap_or_else(|e| die(e)),
    }
    0
}
