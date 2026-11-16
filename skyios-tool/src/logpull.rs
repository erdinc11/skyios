use crate::util::{die, write};
use std::process::Command;

pub fn adb(serial: Option<&str>, args: &[&str]) -> Vec<u8> {
    let mut c = Command::new("adb");
    if let Some(s) = serial {
        c.args(["-s", s]);
    }
    let out = c.args(args).output().unwrap_or_else(|e| die(format!("adb: {e}")));
    if !out.status.success() {
        die(format!("adb {} failed: {}", args.join(" "), out.status));
    }
    out.stdout
}

pub fn adb_text(serial: Option<&str>, args: &[&str]) -> String {
    String::from_utf8_lossy(&adb(serial, args)).trim().replace('\r', "")
}

pub fn main(args: &[String]) -> i32 {
    let (serial, rest) = match args {
        [flag, s, rest @ ..] if flag == "-s" => (Some(s.as_str()), rest),
        _ => (None, args),
    };
    let Some(out) = rest.first() else {
        eprintln!("usage: skyios log-pull [-s SERIAL] OUT.txt   (pull the 4 MiB kernel log ring from userdata via recovery adb)");
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
    let skip = bytes / 4096 - 1025;
    let area = adb(serial, &["exec-out", &format!("dd if={dev} bs=4096 skip={skip} count=1025 2>/dev/null")]);
    let bin = format!("{out}.bin");
    write(&bin, &area);
    eprintln!("{dev}: {bytes} bytes, read blocks {skip}.. ({} bytes)", area.len());
    crate::log::main(&[bin, out.clone()])
}
