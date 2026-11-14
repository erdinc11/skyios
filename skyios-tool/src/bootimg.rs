use crate::util::{cstr_at, die, read, u32le, u64le, write};

pub fn main(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: skyios unpack-bootv2 BOOT.img OUTDIR");
        return 2;
    }
    let (src, out) = (&args[0], &args[1]);
    std::fs::create_dir_all(out).unwrap_or_else(|e| die(format!("{out}: {e}")));
    let b = read(src);
    if !b.starts_with(b"ANDROID!") {
        die("not an Android boot image");
    }
    let h: Vec<u32> = (0..10).map(|i| u32le(&b, 8 + 4 * i)).collect();
    let (ks, ka, rs, ra, ss, _sa, ta, ps, ver, osv) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8], h[9]);
    let cmd = cstr_at(&b[64..64 + 512]) + &cstr_at(&b[608..608 + 1024]);
    let mut info: Vec<(String, String)> = vec![
        ("kernel_size".into(), ks.to_string()),
        ("kernel_addr".into(), format!("{ka:#x}")),
        ("ramdisk_size".into(), rs.to_string()),
        ("ramdisk_addr".into(), format!("{ra:#x}")),
        ("second_size".into(), ss.to_string()),
        ("tags_addr".into(), format!("{ta:#x}")),
        ("page_size".into(), ps.to_string()),
        ("header_version".into(), ver.to_string()),
        ("os_version".into(), format!("{osv:#x}")),
        ("name".into(), cstr_at(&b[48..64])),
        ("cmdline".into(), cmd),
    ];
    let ps = ps as usize;
    let rnd = |n: usize| n.div_ceil(ps) * ps;
    let mut parts = vec![("kernel", ks as usize), ("ramdisk", rs as usize), ("second", ss as usize)];
    if ver >= 1 {
        parts.push(("recovery_dtbo", u32le(&b, 1632) as usize));
        info.push(("header_size".into(), u32le(&b, 1644).to_string()));
    }
    if ver >= 2 {
        parts.push(("dtb", u32le(&b, 1648) as usize));
        info.push(("dtb_addr".into(), format!("{:#x}", u64le(&b, 1652))));
    }
    let mut off = ps;
    for (name, sz) in parts {
        if sz != 0 {
            write(format!("{out}/{name}"), &b[off.min(b.len())..(off + sz).min(b.len())]);
        }
        info.push((format!("{name}_offset"), off.to_string()));
        off += rnd(sz);
    }
    let text: String = info.iter().map(|(k, v)| format!("{k}: {v}\n")).collect();
    write(format!("{out}/header.txt"), text.as_bytes());
    println!("{text}");
    0
}
