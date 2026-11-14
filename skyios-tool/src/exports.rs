use crate::util::die;
use std::collections::HashSet;
use std::path::Path;

const SETS: [&str; 7] = ["BSDKernel", "IOKit", "Libkern", "Mach", "MACFramework", "Unsupported", "Private"];

pub fn main(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: skyios check-exports XNU_SRC ALLSYMBOLS [PLATFORM]");
        return 2;
    }
    let (src, allsyms) = (&args[0], &args[1]);
    let platform = args.get(2).map(|s| s.as_str()).unwrap_or("MacOSX");
    let text = std::fs::read_to_string(allsyms).unwrap_or_else(|e| die(format!("{allsyms}: {e}")));
    let have: HashSet<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let mut missing = 0;
    for s in SETS {
        for name in [format!("{s}.exports"), format!("{s}.arm64.exports"), format!("{s}.arm64.{platform}.exports")] {
            let path = Path::new(src).join("config").join(&name);
            let Ok(body) = std::fs::read_to_string(&path) else { continue };
            for line in body.lines() {
                let e = line.split('#').next().unwrap().trim();
                if e.is_empty() {
                    continue;
                }
                let target = match e.split_once(':') {
                    Some((_, t)) => t.trim(),
                    None => e,
                };
                let ok = match target.strip_suffix('*') {
                    Some(prefix) => have.iter().any(|h| h.starts_with(prefix)),
                    None => have.contains(target),
                };
                if !ok {
                    println!("{name}: {e}");
                    missing += 1;
                }
            }
        }
    }
    eprintln!("{missing} missing");
    (missing != 0) as i32
}
