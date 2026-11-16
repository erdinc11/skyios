use crate::util::{die, read, write};

// Container for bsd_init.c sweet_tool_install() (xnu 0110), passed to the
// kernel with mkpayload --ramdisk. Entries in order: "SKYTOOL1", u32 size,
// u32 mode, 112-byte NUL-terminated destination path, then the bytes, padded
// to 16 bytes; a directory is mode S_IFDIR | 0755 with no bytes. The whole
// image is padded to 16 KiB. The first file is what sweetrun= executes.
const HDR: usize = 128;
const PATH_MAX: usize = 112;
const S_IFDIR: u32 = 0o040000;

const USAGE: &str = "usage: skyios mktool OUT ENTRY...
  ENTRY = SRC=/dest/path (file, mode 0755) | /dest/dir/ (directory)";

fn entry(img: &mut Vec<u8>, dest: &str, mode: u32, data: &[u8]) {
    if !dest.starts_with('/') || dest.len() >= PATH_MAX {
        die(format!("{dest}: want an absolute path shorter than {PATH_MAX} bytes"));
    }
    let mut h = [0u8; HDR];
    h[..8].copy_from_slice(b"SKYTOOL1");
    h[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    h[12..16].copy_from_slice(&mode.to_le_bytes());
    h[16..16 + dest.len()].copy_from_slice(dest.as_bytes());
    img.extend_from_slice(&h);
    img.extend_from_slice(data);
    img.resize(img.len().div_ceil(16) * 16, 0);
}

pub fn main(args: &[String]) -> i32 {
    let Some((out, entries)) = args.split_first().filter(|(_, e)| !e.is_empty()) else {
        die(USAGE)
    };
    let mut img = Vec::new();
    for e in entries {
        if let Some((src, dest)) = e.split_once('=') {
            let data = read(src);
            entry(&mut img, dest, 0o100755, &data);
            println!("file {dest}: {} bytes", data.len());
        } else if let Some(dir) = e.strip_suffix('/') {
            entry(&mut img, dir, S_IFDIR | 0o755, &[]);
            println!("dir {dir}");
        } else {
            die(USAGE);
        }
    }
    img.resize(img.len().div_ceil(0x4000) * 0x4000, 0);
    write(out, &img);
    println!("{out}: {} bytes", img.len());
    0
}
