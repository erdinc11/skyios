use crate::util::die;
use socket2::{Domain, Socket, Type};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

const MAC: &str = "02:00:5e:77:ee:01";
const USB_NAME: &str = "Sweet iOS (USB Ethernet)";
const HOST_IP: &str = "192.168.7.1";
const PORT: u16 = 5140;
const PF_ANCHOR: &str = "com.apple/250.skyios";

fn output(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn run(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).status().map(|s| s.success()).unwrap_or(false)
}

fn sudo(args: &[&str]) {
    let ok = Command::new("sudo")
        .args(args)
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        die(format!("sudo {} failed", args.join(" ")));
    }
}

fn mac_interface() -> Option<String> {
    let reg = output("ioreg", &["-r", "-n", USB_NAME, "-l", "-w0"]);
    reg.lines()
        .find(|l| l.contains("\"BSD Name\""))
        .and_then(|l| l.split('"').nth(3))
        .map(str::to_string)
}

fn linux_interface() -> Option<String> {
    output("ip", &["-o", "link"])
        .lines()
        .find(|l| l.to_lowercase().contains(MAC))
        .and_then(|l| l.split(": ").nth(1))
        .map(|n| n.split('@').next().unwrap_or(n).to_string())
}

fn wait_for(find: fn() -> Option<String>) -> String {
    loop {
        if let Some(i) = find() {
            return i;
        }
        sleep(Duration::from_secs(1));
    }
}

fn setup_macos() -> String {
    let ifname = wait_for(mac_interface);
    sudo(&["ifconfig", &ifname, "inet", HOST_IP, "netmask", "255.255.255.0", "up"]);
    let uplink = output("route", &["-n", "get", "default"])
        .lines()
        .find_map(|l| l.trim().strip_prefix("interface:").map(|s| s.trim().to_string()));
    if let Some(up) = uplink {
        sudo(&["sysctl", "-w", "net.inet.ip.forwarding=1"]);
        let rule = format!("nat on {up} inet from 192.168.7.0/24 to any -> ({up})\n");
        let ok = Command::new("sudo")
            .args(["pfctl", "-a", PF_ANCHOR, "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| {
                c.stdin.take().map(|mut i| i.write_all(rule.as_bytes()));
                c.wait()
            })
            .map(|s| s.success())
            .unwrap_or(false);
        let _ = Command::new("sudo").args(["pfctl", "-E"]).stderr(std::process::Stdio::null()).status();
        eprintln!("NAT 192.168.7.0/24 -> {up} (pf anchor {PF_ANCHOR}){}", if ok { "" } else { " failed" });
    }
    ifname
}

fn setup_linux() -> String {
    let ifname = wait_for(linux_interface);
    run("nmcli", &["dev", "set", &ifname, "managed", "no"]);
    sudo(&["ip", "addr", "replace", &format!("{HOST_IP}/24"), "dev", &ifname]);
    sudo(&["ip", "link", "set", &ifname, "up"]);
    ifname
}

pub fn main(args: &[String]) -> i32 {
    if args.first().is_some_and(|a| a == "-h" || a == "--help") {
        eprintln!("usage: skyios usbnet [LOGFILE]   (wait for the phone's USB Ethernet and stream the live kernel UDP console)");
        return 0;
    }
    eprintln!("waiting for the phone's USB Ethernet ({MAC})...");
    let ifname = if cfg!(target_os = "macos") { setup_macos() } else { setup_linux() };
    eprintln!("{ifname} up: {HOST_IP} <-> phone 192.168.7.2; live log on udp/{PORT}");
    let s = Socket::new(Domain::IPV4, Type::DGRAM, None).unwrap_or_else(|e| die(e));
    s.set_reuse_address(true).unwrap_or_else(|e| die(e));
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT).into()).unwrap_or_else(|e| die(format!("bind {PORT}: {e}")));
    let s: UdpSocket = s.into();
    let mut file = args.first().map(|p| OpenOptions::new().create(true).append(true).open(p).unwrap_or_else(|e| die(format!("{p}: {e}"))));
    let mut buf = vec![0u8; 65536];
    let mut out = std::io::stdout().lock();
    loop {
        let n = s.recv(&mut buf).unwrap_or_else(|e| die(e));
        if let Some(f) = file.as_mut() {
            let _ = f.write_all(&buf[..n]);
        }
        if out.write_all(&buf[..n]).and_then(|_| out.flush()).is_err() {
            return 0;
        }
    }
}
