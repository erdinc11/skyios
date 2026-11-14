use crate::util::{die, parse, put_u64, py_repr, read, u32le, u64le, usage_error, which, write};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const FB_BASE: u64 = 0x9c000000;
const FB_W: usize = 1080;
const FB_H: usize = 2400;
const FB_STRIDE: usize = FB_W * 4;
const PAGE: u64 = 0x4000;
const STUB_PA: u64 = 0x40200000;
const BOOT_LINE_LENGTH: usize = 608;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

fn install_sigint() {
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as *const () as libc::sighandler_t);
    }
}

fn check_qemu_dependency(cpu: &str) -> bool {
    let Some(qemu) = which("qemu-system-aarch64") else {
        eprintln!("[!] Error: qemu-system-aarch64 not found in PATH.");
        eprintln!("    Install instructions:");
        eprintln!("      Arch Linux:    sudo pacman -S qemu-system-aarch64");
        eprintln!("      Ubuntu/Debian: sudo apt install qemu-system-arm");
        eprintln!("      Fedora:         sudo dnf install qemu-system-aarch64");
        eprintln!("      macOS:          brew install qemu");
        return false;
    };
    match Command::new(&qemu).arg("--version").output() {
        Ok(o) if o.status.success() => {
            let v = String::from_utf8_lossy(&o.stdout);
            eprintln!("[*] Found QEMU: {}", v.lines().next().unwrap_or(""));
        }
        Ok(o) => {
            eprintln!("[!] Warning: Failed to query QEMU version: exit {}", o.status);
            return false;
        }
        Err(e) => {
            eprintln!("[!] Warning: Failed to query QEMU version: {e}");
            return false;
        }
    }
    if let Ok(o) = Command::new(&qemu).args(["-M", "virt", "-cpu", "help"]).output() {
        if o.status.success() {
            if String::from_utf8_lossy(&o.stdout).contains(cpu) {
                eprintln!("[*] CPU model {cpu} supported");
            } else {
                eprintln!("[!] Warning: CPU model {cpu} not explicitly listed in qemu-system-aarch64 -cpu help");
            }
        }
    }
    true
}

fn check_kernel_tools(kernel: &str) -> bool {
    if which("llvm-nm").is_none() {
        eprintln!("[!] Error: llvm-nm not found in PATH, required for --kernel font extraction.");
        eprintln!("    Install instructions:");
        eprintln!("      Arch Linux:    sudo pacman -S llvm");
        eprintln!("      Ubuntu/Debian: sudo apt install llvm");
        eprintln!("      Fedora:         sudo dnf install llvm");
        eprintln!("      macOS:          brew install llvm");
        return false;
    }
    if !Path::new(kernel).is_file() {
        eprintln!("[!] Error: kernel file does not exist: {kernel}");
        return false;
    }
    true
}

fn build_image(payload: &[u8]) -> (u64, Vec<u8>, u64, u64) {
    if &payload[..8] != b"SWEETXNU" || u32le(payload, 8) != 1 {
        die("not a SWEETXNU v1 payload");
    }
    let nsegs = u32le(payload, 12) as usize;
    let h: Vec<u64> = (0..7).map(|i| u64le(payload, 0x10 + 8 * i)).collect();
    let (virt_base, entry_va, adt_off, adt_size, kernel_end, phys_base, mem_size) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6]);
    let cmdline = &payload[0x48..0x48 + BOOT_LINE_LENGTH];
    let delta = phys_base.wrapping_sub(virt_base);
    let adt_pa = kernel_end.wrapping_add(delta);
    let ba_pa = (adt_pa + adt_size + PAGE - 1) & !(PAGE - 1);
    let top = ba_pa + PAGE;
    let mut img = vec![0u8; (top - phys_base) as usize];
    for i in 0..nsegs {
        let e = 0x2a8 + 32 * i;
        let (vmaddr, foff, fsize) = (u64le(payload, e), u64le(payload, e + 16) as usize, u64le(payload, e + 24) as usize);
        let o = (vmaddr.wrapping_add(delta) - phys_base) as usize;
        img[o..o + fsize].copy_from_slice(&payload[foff..foff + fsize]);
    }
    let a = (adt_pa - phys_base) as usize;
    img[a..a + adt_size as usize].copy_from_slice(&payload[adt_off as usize..(adt_off + adt_size) as usize]);
    let mut ba = vec![];
    ba.extend(2u16.to_le_bytes());
    ba.extend(2u16.to_le_bytes());
    ba.extend([0u8; 4]);
    for v in [virt_base, phys_base, mem_size, top, FB_BASE, 1, FB_STRIDE as u64, FB_W as u64, FB_H as u64, 32] {
        ba.extend(v.to_le_bytes());
    }
    ba.extend(0u32.to_le_bytes());
    ba.extend([0u8; 4]);
    ba.extend(adt_pa.wrapping_sub(delta).to_le_bytes());
    ba.extend((adt_size as u32).to_le_bytes());
    ba.extend(cmdline);
    ba.extend([0u8; 4]);
    ba.extend(0u64.to_le_bytes());
    ba.extend(mem_size.to_le_bytes());
    assert_eq!(ba.len(), 736);
    let b = (ba_pa - phys_base) as usize;
    img[b..b + ba.len()].copy_from_slice(&ba);
    (phys_base, img, ba_pa, entry_va.wrapping_add(delta))
}

fn stub(ba_pa: u64, entry_pa: u64) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    for (i, w) in [0x58000080u32, 0x580000a1, 0xd61f0020, 0xd503201f].iter().enumerate() {
        v[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    put_u64(&mut v, 16, ba_pa);
    put_u64(&mut v, 24, entry_pa);
    v
}

fn png_chunk(t: &[u8], d: &[u8]) -> Vec<u8> {
    let mut c = (d.len() as u32).to_be_bytes().to_vec();
    c.extend(t);
    c.extend(d);
    let mut h = crc32fast::Hasher::new();
    h.update(t);
    h.update(d);
    c.extend(h.finalize().to_be_bytes());
    c
}

fn write_png(path: &str, raw: &[u8]) {
    let mut rows = Vec::with_capacity(FB_H * (FB_W * 3 + 1));
    for y in 0..FB_H {
        rows.push(0);
        for px in raw[y * FB_STRIDE..(y + 1) * FB_STRIDE].chunks(4) {
            rows.extend([px[2], px[1], px[0]]);
        }
    }
    let mut z = ZlibEncoder::new(vec![], Compression::new(6));
    z.write_all(&rows).unwrap();
    let mut ihdr = vec![];
    ihdr.extend((FB_W as u32).to_be_bytes());
    ihdr.extend((FB_H as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(png_chunk(b"IHDR", &ihdr));
    png.extend(png_chunk(b"IDAT", &z.finish().unwrap()));
    png.extend(png_chunk(b"IEND", b""));
    write(path, &png);
}

fn payload_data(payload: &[u8], va: u64, size: u64) -> Result<Vec<u8>, String> {
    for i in 0..u32le(payload, 12) as usize {
        let e = 0x2a8 + 32 * i;
        let (vmaddr, foff, fsize) = (u64le(payload, e), u64le(payload, e + 16), u64le(payload, e + 24));
        if vmaddr <= va && va + size <= vmaddr + fsize {
            let o = (foff + va - vmaddr) as usize;
            return Ok(payload[o..o + size as usize].to_vec());
        }
    }
    Err(format!("VA {va:#x}+{size:#x} not in initialized payload segments"))
}

fn decode_screen(raw: &[u8], font: &[u8]) -> (String, usize) {
    let glyphs: HashMap<&[u8], char> = (32u8..127).map(|c| (&font[c as usize * 16..(c as usize + 1) * 16], c as char)).collect();
    let mut rows = vec![];
    let mut unknown = 0;
    for row in 0..FB_H / 32 {
        let mut chars = String::new();
        for col in 0..FB_W / 16 {
            let mut glyph = [0u8; 16];
            for (y, g) in glyph.iter_mut().enumerate() {
                let off = (row * 32 + y * 2) * FB_STRIDE + col * 16 * 4;
                for x in 0..8 {
                    if raw[off + x * 8..off + x * 8 + 4] == [0xff; 4] {
                        *g |= 1 << x;
                    }
                }
            }
            match glyphs.get(&glyph[..]) {
                Some(c) => chars.push(*c),
                None => {
                    chars.push('?');
                    unknown += 1;
                }
            }
        }
        rows.push(chars.trim_end().to_string());
    }
    (rows.join("\n").trim_end().to_string() + "\n", unknown)
}

fn screen_status(screen: &str, unknown: usize) -> &'static str {
    if screen.contains("panic(cpu ") || screen.contains("SWEET EARLY PANIC:") {
        "panic"
    } else if unknown != 0 {
        "incomplete_decode"
    } else if !screen.trim().is_empty() {
        "no_panic_observed"
    } else {
        "blank_screen"
    }
}

fn qmp(sock: &str, cmds: &[String], pmemsave: (u64, usize, String)) -> Vec<String> {
    let s = UnixStream::connect(sock).unwrap_or_else(|e| die(format!("{sock}: {e}")));
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).unwrap_or_else(|e| die(e));
    let mut call = |req: Value| -> Value {
        writeln!(w, "{req}").unwrap_or_else(|e| die(e));
        loop {
            let mut l = String::new();
            if r.read_line(&mut l).unwrap_or(0) == 0 {
                die("QMP connection closed");
            }
            let v: Value = serde_json::from_str(&l).unwrap_or_else(|e| die(format!("QMP: {e}")));
            if v.get("return").is_some() || v.get("error").is_some() {
                return v;
            }
        }
    };
    call(json!({"execute": "qmp_capabilities", "arguments": {}}));
    let mut out = vec![];
    for c in cmds {
        let v = call(json!({"execute": "human-monitor-command", "arguments": {"command-line": c}}));
        if let Some(e) = v.get("error") {
            eprintln!("qmp {}: {}", py_repr(c), pyval(e));
        }
        out.push(v.get("return").and_then(|r| r.as_str()).unwrap_or("").to_string());
    }
    let v = call(json!({"execute": "pmemsave", "arguments": {"val": pmemsave.0, "size": pmemsave.1, "filename": pmemsave.2}}));
    if let Some(e) = v.get("error") {
        eprintln!("qmp pmemsave: {}", pyval(e));
    }
    out
}

fn pyval(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => py_repr(s),
        Value::Array(a) => format!("[{}]", a.iter().map(pyval).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!("{{{}}}", o.iter().map(|(k, v)| format!("{}: {}", py_repr(k), pyval(v))).collect::<Vec<_>>().join(", ")),
    }
}

fn stream_serial(path: &str, pos: u64) -> u64 {
    let Ok(mut f) = std::fs::File::open(path) else { return pos };
    if f.seek(SeekFrom::Start(pos)).is_err() {
        return pos;
    }
    let mut data = vec![];
    if std::io::Read::read_to_end(&mut f, &mut data).is_err() || data.is_empty() {
        return pos;
    }
    let mut out = std::io::stdout();
    let _ = out.write_all(String::from_utf8_lossy(&data).as_bytes());
    let _ = out.flush();
    pos + data.len() as u64
}

fn result_json(fields: &[(&str, String)], pretty: bool) -> String {
    let items: Vec<String> = fields.iter().map(|(k, v)| format!("\"{k}\": {v}")).collect();
    if pretty {
        format!("{{\n  {}\n}}", items.join(",\n  "))
    } else {
        format!("{{{}}}", items.join(", "))
    }
}

fn q(s: &str) -> String {
    format!("\"{s}\"")
}

pub fn main(args: &[String]) -> i32 {
    let o = parse(
        "qemu",
        args,
        &["--secs", "--hmp", "--cpu", "--kernel"],
        &["--gdb", "--fail-on-panic", "--live-serial", "--check-env"],
    );
    let cpu = o.get("--cpu").unwrap_or("cortex-a76").to_string();
    let secs: f64 = o
        .get("--secs")
        .map(|s| s.parse().unwrap_or_else(|_| usage_error("qemu", &format!("argument --secs: invalid float value: '{s}'"))))
        .unwrap_or(20.0);
    let hmp = o.all("--hmp");
    let kernel = o.get("--kernel").map(String::from);

    if o.flag("--check-env") {
        let ok = check_qemu_dependency(&cpu);
        if which("llvm-nm").is_some() {
            eprintln!("[*] llvm-nm found in PATH");
        } else {
            eprintln!("[!] Optional: llvm-nm not found (needed only when using --kernel for text decoding)");
        }
        return if ok { 0 } else { 1 };
    }
    if o.pos.len() != 2 {
        usage_error("qemu", "payload and outdir are required unless --check-env is specified");
    }
    if o.flag("--fail-on-panic") && kernel.is_none() {
        usage_error("qemu", "--fail-on-panic requires --kernel");
    }
    if !check_qemu_dependency(&cpu) {
        return 1;
    }
    if let Some(k) = &kernel {
        if !check_kernel_tools(k) {
            return 1;
        }
    }

    let payload = read(&o.pos[0]);
    let font = kernel.as_ref().map(|k| {
        let out = Command::new("llvm-nm").args(["--defined-only", k]).output().unwrap_or_else(|e| die(format!("llvm-nm: {e}")));
        if !out.status.success() {
            die("llvm-nm failed");
        }
        let syms = String::from_utf8_lossy(&out.stdout).into_owned();
        let fonts: Vec<u64> = syms
            .lines()
            .filter(|l| l.split_whitespace().last() == Some("_iso_font"))
            .map(|l| u64::from_str_radix(l.split_whitespace().next().unwrap(), 16).unwrap_or_else(|e| die(e)))
            .collect();
        if fonts.len() != 1 {
            usage_error("qemu", "kernel must contain exactly one _iso_font symbol");
        }
        payload_data(&payload, fonts[0], 256 * 16).unwrap_or_else(|e| die(e))
    });
    std::fs::create_dir_all(&o.pos[1]).unwrap_or_else(|e| die(e));
    let out = std::path::absolute(&o.pos[1]).unwrap_or_else(|e| die(e)).to_string_lossy().into_owned();

    let (phys_base, img, ba_pa, entry_pa) = build_image(&payload);
    write(format!("{out}/ram.bin"), &img);
    drop(img);
    write(format!("{out}/stub.bin"), &stub(ba_pa, entry_pa));
    eprintln!("phys_base={phys_base:#x} boot_args={ba_pa:#x} entry={entry_pa:#x}");

    let mon = format!("{out}/mon.sock");
    let _ = std::fs::remove_file(&mon);
    let mem_size = u64le(&payload, 0x40);
    let need = (phys_base + mem_size) as i128 - 0x40000000;
    let ram_gib = 4.max((need + (1 << 30) - 1).div_euclid(1 << 30));
    let mut cmd: Vec<String> = [
        "-M", "virt,gic-version=3", "-cpu", &cpu, "-smp", "1", "-m", &format!("{ram_gib}G"),
        "-nographic", "-serial", &format!("file:{out}/serial.txt"), "-qmp", &format!("unix:{mon},server,nowait"),
        "-d", "int,guest_errors,unimp", "-D", &format!("{out}/qemu.log"), "-no-reboot",
        "-device", &format!("loader,file={out}/ram.bin,addr={phys_base:#x},force-raw=on"),
        "-device", &format!("loader,file={out}/stub.bin,addr={STUB_PA:#x},force-raw=on"),
        "-device", &format!("loader,addr={STUB_PA:#x},cpu-num=0"),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if o.flag("--gdb") {
        cmd.extend(["-S", "-gdb", "tcp::1234"].map(String::from));
    }

    install_sigint();
    let rc = run(&o, &cmd, &out, &mon, &hmp, font.as_deref(), secs);
    let _ = std::fs::remove_file(format!("{out}/ram.bin"));
    rc
}

fn run(o: &crate::util::Opts, cmd: &[String], out: &str, mon: &str, hmp: &[String], font: Option<&[u8]>, secs: f64) -> i32 {
    let interrupted = |p: &mut std::process::Child| {
        eprintln!("\n[!] Interrupted by user, stopping QEMU...");
        if let Ok(None) = p.try_wait() {
            let _ = p.kill();
            let _ = p.wait();
        }
        130
    };
    let mut p = Command::new("qemu-system-aarch64")
        .args(cmd)
        .stdin(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| die(format!("qemu-system-aarch64: {e}")));
    if o.flag("--gdb") {
        eprintln!("gdb stub on :1234; QMP socket: {mon}");
        let _ = p.wait();
        if INTERRUPTED.load(Ordering::SeqCst) {
            return interrupted(&mut p);
        }
        return 0;
    }
    let serial = format!("{out}/serial.txt");
    let mut pos = 0;
    let t0 = Instant::now();
    while matches!(p.try_wait(), Ok(None)) && t0.elapsed().as_secs_f64() < secs {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return interrupted(&mut p);
        }
        if o.flag("--live-serial") {
            pos = stream_serial(&serial, pos);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if INTERRUPTED.load(Ordering::SeqCst) {
        return interrupted(&mut p);
    }
    if o.flag("--live-serial") {
        stream_serial(&serial, pos);
    }

    let mut status;
    let mut fields: Vec<(&str, String)>;
    match p.try_wait() {
        Ok(None) => {
            let mut cmds = vec!["stop".to_string(), "info registers".to_string()];
            cmds.extend(hmp.iter().cloned());
            let res = qmp(mon, &cmds, (FB_BASE, FB_STRIDE * FB_H, format!("{out}/fb.raw")));
            let mut regs = res[1].clone();
            for (c, r) in hmp.iter().zip(&res[2..]) {
                regs += &format!("\n(qemu) {c}\n{r}");
            }
            write(format!("{out}/regs.txt"), regs.as_bytes());
            let _ = p.kill();
            let _ = p.wait();
            let raw = read(format!("{out}/fb.raw"));
            write_png(&format!("{out}/fb.png"), &raw);
            status = "not_decoded";
            fields = vec![("status", q(status))];
            if let Some(font) = font {
                let (screen, unknown) = decode_screen(&raw, font);
                write(format!("{out}/fb.txt"), screen.as_bytes());
                status = screen_status(&screen, unknown);
                fields = vec![("status", q(status)), ("unknown_glyphs", unknown.to_string()), ("snapshot_only", "true".into())];
            }
            let _ = std::fs::remove_file(format!("{out}/fb.raw"));
        }
        other => {
            let code = match other {
                Ok(Some(s)) => s.code().map(|c| c.to_string()).unwrap_or_else(|| {
                    use std::os::unix::process::ExitStatusExt;
                    format!("-{}", s.signal().unwrap_or(0))
                }),
                _ => "null".into(),
            };
            status = "qemu_exited";
            fields = vec![("status", q(status)), ("qemu_returncode", code.clone())];
            eprintln!("qemu exited early: {code}");
        }
    }
    write(format!("{out}/result.json"), (result_json(&fields, true) + "\n").as_bytes());
    eprintln!("{}", result_json(&fields, false));
    eprintln!("{out}/fb.png  {out}/regs.txt  {out}/qemu.log");
    let fail = o.flag("--fail-on-panic");
    if fail && status == "panic" {
        return 2;
    }
    if fail && (status == "incomplete_decode" || status == "blank_screen") {
        return 3;
    }
    if status == "qemu_exited" {
        return 1;
    }
    0
}
