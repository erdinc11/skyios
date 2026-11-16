use crate::util::{die, read, write};

// Container for bsd_init.c sweet_tool_install() (xnu 0109), passed to the
// kernel with mkpayload --ramdisk: "SKYTOOL1", u32 size, u32 mode, 112-byte
// NUL-terminated destination path, then the file, padded to 16 KiB.
const HDR: usize = 128;
const PATH_MAX: usize = 112;

pub fn main(args: &[String]) -> i32 {
    let [bin, dest, out] = args else {
        die("usage: skyios mktool BINARY DEST_PATH OUT")
    };
    if !dest.starts_with('/') || dest.len() >= PATH_MAX {
        die(format!("{dest}: want an absolute path shorter than {PATH_MAX} bytes"));
    }
    let data = read(bin);
    let mut img = vec![0u8; (HDR + data.len()).div_ceil(0x4000) * 0x4000];
    img[..8].copy_from_slice(b"SKYTOOL1");
    img[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
    img[12..16].copy_from_slice(&0o755u32.to_le_bytes());
    img[16..16 + dest.len()].copy_from_slice(dest.as_bytes());
    img[HDR..HDR + data.len()].copy_from_slice(&data);
    write(out, &img);
    println!("{out}: {} bytes for {dest}", data.len());
    0
}
