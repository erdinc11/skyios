use crate::adt::{self, Node};
use crate::util::{die, read, repo_root, write};
use sha2::{Digest, Sha384};
use std::io::Read;

const PHYS_BASE: u64 = 0xb0000000;
const MEM_SIZE: u64 = 0x4d000000;
const PANIC_LOG_SIZE: u64 = 0x100000;
const HIGH_PHYS_BASE: u64 = 0x100000000;
const HIGH_MEM_SIZE: u64 = 0xfcc00000;
const SOC_BASE: u64 = 0x01000000;

const GICD: u64 = 0x17a00000;
const GICD_SIZE: u64 = 0x10000;
const GICR: u64 = 0x17a60000;
const GICR_SIZE: u64 = 0x100000;
const FB: u64 = 0x9c000000;
const FB_SIZE: u64 = 0x1700000;
const SWEET_ECID: u64 = 0x5357454554000001;
const FB_W: u32 = 1080;
const FB_H: u32 = 2400;
const FB_STRIDE: u32 = 1080 * 4;
const UFS: u64 = 0x01d84000;
const UFS_SIZE: u64 = 0x3000;
const TIMEBASE: u32 = 19200000;
const VTIMER_INTID: u32 = 19;
const QEMU_GICD: u64 = 0x08000000;
const QEMU_GICR: u64 = 0x080a0000;
const QEMU_TIMEBASE: u32 = 62500000;
const QEMU_VTIMER_INTID: u32 = 27;

fn u32b(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn u64b(v: &[u64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn cstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn cstrs(v: &[&str]) -> Vec<u8> {
    v.iter().flat_map(|s| cstr(s)).collect()
}

type Props = Vec<(String, Vec<u8>)>;

fn node(name: &str, props: Props, children: Vec<Node>) -> Node {
    let mut n = Node::default();
    n.props.push(("name".into(), 0, cstr(name)));
    n.props.extend(props.into_iter().map(|(k, v)| (k, 0, v)));
    n.children = children;
    n
}

fn p(k: &str, v: Vec<u8>) -> (String, Vec<u8>) {
    (k.to_string(), v)
}

fn d22ap_product() -> Option<Props> {
    let path = repo_root().join("research/ipsw-x/d22ap.adt");
    if !path.exists() {
        eprintln!("warning: {} missing, no /product node", path.display());
        return None;
    }
    let (root, _) = adt::parse(&read(&path), 0);
    for c in &root.children {
        if c.props.iter().rev().find(|p| p.0 == "name").map(|p| p.2.as_slice()) == Some(b"product\0") {
            return Some(
                c.props
                    .iter()
                    .filter(|(k, f, _)| *f == 0 && k != "name" && k != "AAPL,phandle")
                    .map(|(k, _, v)| (k.clone(), v.clone()))
                    .collect(),
            );
        }
    }
    die(format!("{}: no /product node", path.display()))
}

fn urandom(n: usize) -> Vec<u8> {
    let mut v = vec![0; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut v))
        .unwrap_or_else(|e| die(format!("/dev/urandom: {e}")));
    v
}

pub fn build(qemu: bool, highmem: bool) -> Node {
    let (phys_base, mut mem_size) = if highmem { (HIGH_PHYS_BASE, HIGH_MEM_SIZE) } else { (PHYS_BASE, MEM_SIZE) };
    let panic_base = phys_base + mem_size - PANIC_LOG_SIZE;
    mem_size -= PANIC_LOG_SIZE;
    let (gicd, gicr, timebase) = if qemu { (QEMU_GICD, QEMU_GICR, QEMU_TIMEBASE) } else { (GICD, GICR, TIMEBASE) };
    let seed = urandom(64);
    let mut next = 0u32;
    let mut ph = || {
        next += 1;
        p("AAPL,phandle", u32b(&[next]))
    };

    let chosen_ph = ph();
    let mmap_ph = ph();
    let asmb_ph = ph();
    let chosen = node(
        "chosen",
        vec![
            chosen_ph,
            p("dram-base", u64b(&[phys_base])),
            p("dram-size", u64b(&[mem_size])),
            p("random-seed", seed),
            p("debug-enabled", u32b(&[1])),
            p("embedded-panic-log-size", u32b(&[PANIC_LOG_SIZE as u32])),
            p("firmware-version", cstr("sweet-shim")),
            p("system-firmware-version", cstr("sweet-shim")),
            p("boot-manifest-hash", Sha384::digest(b"sweet boot manifest").to_vec()),
            p("crypto-hash-method", cstr("sha2-384")),
            p("display-scale", u32b(&[3])),
            p("unique-chip-id", u64b(&[SWEET_ECID])),
            p("chip-id", u32b(&[0x8015])),
            p("certificate-production-status", u32b(&[0])),
            p("effective-production-status-ap", u32b(&[0])),
            p("certificate-security-mode", u32b(&[1])),
            p("effective-security-mode-sep", u32b(&[1])),
            p("mac-address-wifi0", vec![0x02, 0x00, 0x5e, 0x77, 0xee, 0x10]),
            p("mac-address-bluetooth0", vec![0x02, 0x00, 0x5e, 0x77, 0xee, 0x11]),
            p("mac-address-ethernet0", vec![0x02, 0x00, 0x5e, 0x77, 0xee, 0x12]),
        ],
        vec![
            node("memory-map", vec![mmap_ph], vec![]),
            node("asmb", vec![asmb_ph, p("lp-sip0", u64b(&[0x6f])), p("lp-sip1", u32b(&[1]))], vec![]),
        ],
    );

    let mut cpu_nodes = vec![];
    for core in 0..if qemu { 1 } else { 8 } {
        cpu_nodes.push(node(
            &format!("cpu{core}"),
            vec![
                ph(),
                p("device_type", cstr("cpu")),
                p("compatible", cstrs(&["qcom,kryo470", "ARM,v8"])),
                p("reg", u32b(&[core << 8])),
                p("cpu-id", u32b(&[core])),
                p("cluster-id", u32b(&[0])),
                p("cluster-type", u32b(&[0])),
                p("die-cluster-id", u32b(&[0])),
                p("cluster-core-id", u32b(&[core])),
                p("l2-cache-id", u32b(&[core])),
                p("l3-cache-id", u32b(&[0])),
                p("state", cstr(if core == 0 { "running" } else { "stopped" })),
                p("timebase-frequency", u32b(&[timebase])),
                p("clock-frequency", u32b(&[if core >= 6 { 2304000000 } else { 1804800000 }])),
                p("bus-frequency", u32b(&[100000000])),
                p("memory-frequency", u32b(&[1866000000])),
                p("peripheral-frequency", u32b(&[timebase])),
                p("fixed-frequency", u32b(&[timebase])),
            ],
            vec![],
        ));
    }
    let cpus = node("cpus", vec![ph(), p("#address-cells", u32b(&[1])), p("#size-cells", u32b(&[0]))], cpu_nodes);

    let gic = node(
        "gic",
        vec![
            ph(),
            p("compatible", cstr("arm,gic-v3")),
            p("interrupt-controller", vec![]),
            p("virtual-timer-intid", u32b(&[if qemu { QEMU_VTIMER_INTID } else { VTIMER_INTID }])),
            p("reg", u64b(&[gicd - SOC_BASE, GICD_SIZE, gicr - SOC_BASE, GICR_SIZE])),
        ],
        vec![],
    );
    let arm_io_props = vec![
        ph(),
        p("device_type", cstr("sweet-io")),
        p("compatible", cstrs(&["sweet-io", "arm-io"])),
        p("#address-cells", u32b(&[2])),
        p("#size-cells", u32b(&[2])),
        p("ranges", u64b(&[0, SOC_BASE, 0x100000000 - SOC_BASE])),
    ];
    let wdt = node("wdt", vec![ph(), p("no-pmu", u32b(&[1]))], vec![]);
    let disp0 = node(
        "disp0",
        vec![
            ph(),
            p("device_type", cstr("display-subsystem")),
            p("compatible", cstr("sweet,display")),
            p("canvas-width", u32b(&[1125])),
            p("canvas-height", u32b(&[2436])),
        ],
        vec![],
    );
    let arm_io = node("arm-io", arm_io_props, vec![gic, wdt, disp0]);

    let fbcon = node(
        "sweet-fbcon",
        vec![
            ph(),
            p("reg", u64b(&[FB, FB_SIZE])),
            p("width", u32b(&[FB_W])),
            p("height", u32b(&[FB_H])),
            p("stride", u32b(&[FB_STRIDE])),
        ],
        vec![],
    );
    let ufs = node(
        "sweet-ufs",
        vec![ph(), p("compatible", cstr("qcom,ufshc")), p("reg", u64b(&[UFS, UFS_SIZE]))],
        vec![],
    );
    let defaults = node("defaults", vec![ph(), p("no-effaceable-storage", u32b(&[1]))], vec![]);
    let buttons = node(
        "buttons",
        vec![
            ph(),
            p("button-names", cstrs(&["volup", "voldown", "hold"])),
            p("home-button-type", u32b(&[2])),
            p("opposed-power-vol-buttons", u32b(&[1])),
        ],
        vec![],
    );
    let product = d22ap_product().map(|props| {
        let mut v = vec![ph()];
        v.extend(props);
        node("product", v, vec![])
    });
    let pram = node("pram", vec![ph(), p("reg", u64b(&[panic_base, PANIC_LOG_SIZE]))], vec![]);

    let mut children = vec![chosen, defaults, cpus, arm_io, fbcon, buttons, pram];
    children.extend(product);
    if !qemu {
        children.push(ufs);
    }
    node(
        "device-tree",
        vec![
            ph(),
            p("compatible", cstrs(&["D22AP", "iPhone10,3", "xiaomi,sweet", "qcom,sm7150", "AppleARM"])),
            p("model", cstr("iPhone10,3")),
            p("target-type", cstr("D22")),
            p("target-sub-type", cstr("D22AP")),
            p("manufacturer", cstr("Apple Inc.")),
            p("#address-cells", u32b(&[2])),
            p("#size-cells", u32b(&[2])),
        ],
        children,
    )
}

pub fn main(args: &[String]) -> i32 {
    let Some(out) = args.first() else {
        eprintln!("usage: skyios mkadt OUT.adt [--dump] [--qemu] [--highmem]");
        return 2;
    };
    let has = |f: &str| args.iter().any(|a| a == f);
    let root = build(has("--qemu"), has("--highmem"));
    let blob = adt::serialize(&root);
    write(out, &blob);
    if has("--dump") {
        adt::dump(&adt::parse(&blob, 0).0, 0, false);
    }
    eprintln!("{out}: {} bytes", blob.len());
    0
}
