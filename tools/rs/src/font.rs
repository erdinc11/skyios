use crate::util::{die, read, u32le, write};
use flate2::read::MultiGzDecoder;
use std::io::Read;

pub fn main(args: &[String]) -> i32 {
    let Some(src) = args.first() else {
        eprintln!("usage: sweet gen-font FONT.psfu.gz   (writes font.h in the current directory)");
        return 2;
    };
    let mut d = vec![];
    MultiGzDecoder::new(&read(src)[..]).read_to_end(&mut d).unwrap_or_else(|e| die(format!("{src}: {e}")));
    let (off, h) = if d.starts_with(b"\x36\x04") {
        (4, d[3] as usize)
    } else {
        if !d.starts_with(b"\x72\xb5\x4a\x86") {
            die("PSF1/PSF2 expected");
        }
        if u32le(&d, 28) != 8 {
            die("font width must be 8");
        }
        (u32le(&d, 8) as usize, u32le(&d, 24) as usize)
    };
    if h != 16 {
        die("font height must be 16");
    }
    let mut out = vec!["static const unsigned char font8x16[95][16] = {".to_string()];
    for c in 0x20..0x7f {
        let g = &d[off + c * h..off + c * h + h];
        out.push(format!("\t{{{}}},", g.iter().map(|b| format!("0x{b:02x}")).collect::<Vec<_>>().join(",")));
    }
    out.push("};".into());
    write("font.h", (out.join("\n") + "\n").as_bytes());
    0
}
