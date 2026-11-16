mod adt;
mod bootimg;
mod crash;
mod exports;
mod font;
mod launchd;
mod log;
mod logpull;
mod mkadt;
mod mkinit;
mod mkiosroot;
mod mkshim;
mod mktool;
mod mkpayload;
mod plistpy;
mod qemu;
mod remotezip;
mod sparsediff;
mod tweaks;
mod udp;
mod usbnet;
mod util;
mod validate;
mod xnucc;

const USAGE: &str = "usage: skyios COMMAND ARGS...

  mkadt OUT.adt [--dump] [--qemu] [--highmem]
  mkpayload KERNEL ADT OUT [--cmdline \"...\"] [--ramdisk IMAGE]
  mkshim SHIM PAYLOAD DTB OUT.img [--cmdline STR] [--image OUT.Image]
  mkinit OBJECT OUT [ENTITLEMENTS.plist]
  mktool OUT SRC=/DEST... | /DIR/...
  adt dump FILE [--hex] | adt unwrap IM4P OUT
  patch-launchd-plist IN.plist OUT.plist LABEL [LABEL ...]
  mkiosroot OS.dmg SystemOS.dmg AppOS.dmg OUT.img [SIZE]
  root-tweaks MOUNTED_ROOT
  sparsediff OLD.img NEW.img OUT.simg [--block 4096]
  crash CACHE ROOT [REPORT.txt]
  log AREA.bin [OUT.txt]
  log-pull [-s SERIAL] OUT.txt
  usbnet [LOGFILE]
  unpack-bootv2 BOOT.img OUTDIR
  remotezip URL [PATTERN OUTDIR]
  check-exports XNU_SRC ALLSYMBOLS [PLATFORM]
  validate-repo [--verbose]
  xnu-cc LOG BUILDTREE [FILTER ...] [--per-file]
  qemu PAYLOAD OUTDIR [--secs N] [--gdb] [--live-serial] [--kernel K] [--fail-on-panic] [--hmp CMD] [--cpu CPU] | qemu --check-env
  udp-listen PORT
  gen-font FONT.psfu.gz";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first() else {
        eprintln!("{USAGE}");
        std::process::exit(2)
    };
    let args = &argv[1..];
    let rc = match cmd.as_str() {
        "mkadt" => mkadt::main(args),
        "mkpayload" => mkpayload::main(args),
        "mkshim" => mkshim::main(args),
        "mkinit" => mkinit::main(args),
        "mktool" => mktool::main(args),
        "adt" => adt::main(args),
        "patch-launchd-plist" => launchd::main(args),
        "root-tweaks" => tweaks::main(args),
        "mkiosroot" => mkiosroot::main(args),
        "sparsediff" => sparsediff::main(args),
        "crash" => crash::main(args),
        "log" => log::main(args),
        "log-pull" => logpull::main(args),
        "usbnet" => usbnet::main(args),
        "unpack-bootv2" => bootimg::main(args),
        "remotezip" => remotezip::main(args),
        "check-exports" => exports::main(args),
        "validate-repo" => validate::main(args),
        "xnu-cc" => xnucc::main(args),
        "qemu" => qemu::main(args),
        "udp-listen" => udp::main(args),
        "gen-font" => font::main(args),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("skyios: unknown command {cmd}\n{USAGE}");
            2
        }
    };
    std::process::exit(rc)
}
