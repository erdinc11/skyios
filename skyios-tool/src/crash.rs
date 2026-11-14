use crate::util::die;
use regex::Regex;
use std::io::Read;
use std::process::Command;

const CACHE_LO: i128 = 0x180000000;
const CACHE_HI: i128 = 0x300000000;

fn a2s(cache: &str, addr: i128) -> String {
    let out = Command::new("ipsw").args(["dyld", "a2s", cache, &format!("{addr:#x}")]).output();
    let stdout = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    stdout
        .lines()
        .filter(|l| l.starts_with("0x"))
        .next_back()
        .and_then(|l| l.split_once(": ").map(|x| x.1.to_string()))
        .unwrap_or_else(|| "?".into())
}

fn hex(s: &str) -> i128 {
    i128::from_str_radix(&s[2..], 16).unwrap_or(i128::MAX)
}

pub fn main(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: skyios crash CACHE ROOT [REPORT.txt]");
        return 2;
    }
    let cache = &args[0];
    let text = match args.get(2) {
        Some(p) => std::fs::read_to_string(p).unwrap_or_else(|e| die(format!("{p}: {e}"))),
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).unwrap_or_else(|e| die(e));
            s
        }
    };
    let head = Regex::new(r"slide (0x[0-9a-f]+) mh (0x[0-9a-f]+)").unwrap();
    let addr = Regex::new(r"(?:pc|lr|bt|\s)(?: )?(0x[0-9a-f]{6,})").unwrap();
    let (mut slide, mut mh) = (0i128, 0i128);
    for line in text.lines() {
        if let Some(m) = head.captures(line) {
            slide = hex(&m[1]);
            mh = hex(&m[2]);
            println!("{line}");
            continue;
        }
        println!("{line}");
        for m in addr.captures_iter(line) {
            let a = &m[1];
            let v = hex(a);
            if CACHE_LO <= v - slide && v - slide < CACHE_HI {
                println!("    {a} = cache {:#x} {}", v - slide, a2s(cache, v - slide));
            } else if mh != 0 && mh <= v && v < mh + 0x10000000 {
                println!("    {a} = main binary {:#x}", v - mh + 0x100000000);
            }
        }
    }
    0
}
