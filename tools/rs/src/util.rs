use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    std::process::exit(1)
}

pub fn read(path: impl AsRef<Path>) -> Vec<u8> {
    let p = path.as_ref();
    std::fs::read(p).unwrap_or_else(|e| die(format!("{}: {e}", p.display())))
}

pub fn write(path: impl AsRef<Path>, data: &[u8]) {
    let p = path.as_ref();
    std::fs::write(p, data).unwrap_or_else(|e| die(format!("{}: {e}", p.display())))
}

pub fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

pub fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

pub fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

pub fn put_u32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn put_u64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

pub fn cstr_at(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

pub fn repo_root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        for a in exe.ancestors() {
            if a.join("tools/rs/Cargo.toml").is_file() {
                return a.to_path_buf();
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    })
}

pub fn py_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == q => {
                out.push('\\');
                out.push(c)
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

pub fn py_list(items: &[&str]) -> String {
    format!("[{}]", items.iter().map(|s| py_repr(s)).collect::<Vec<_>>().join(", "))
}

pub struct Opts {
    pub pos: Vec<String>,
    vals: HashMap<String, Vec<String>>,
    flags: HashSet<String>,
}

impl Opts {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.vals.get(k).and_then(|v| v.last()).map(|s| s.as_str())
    }
    pub fn all(&self, k: &str) -> Vec<String> {
        self.vals.get(k).cloned().unwrap_or_default()
    }
    pub fn flag(&self, k: &str) -> bool {
        self.flags.contains(k)
    }
}

pub fn usage_error(prog: &str, msg: &str) -> ! {
    eprintln!("sweet {prog}: error: {msg}");
    std::process::exit(2)
}

pub fn parse(prog: &str, args: &[String], valued: &[&str], flags: &[&str]) -> Opts {
    let mut o = Opts { pos: vec![], vals: HashMap::new(), flags: HashSet::new() };
    let mut it = args.iter();
    let mut rest = false;
    while let Some(a) = it.next() {
        if rest || !a.starts_with("--") || a == "--" {
            if a == "--" && !rest {
                rest = true;
                continue;
            }
            o.pos.push(a.clone());
            continue;
        }
        let (k, inline) = match a.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (a.clone(), None),
        };
        if valued.contains(&k.as_str()) {
            let v = inline
                .or_else(|| it.next().cloned())
                .unwrap_or_else(|| usage_error(prog, &format!("argument {k}: expected one argument")));
            o.vals.entry(k).or_default().push(v);
        } else if flags.contains(&k.as_str()) && inline.is_none() {
            o.flags.insert(k);
        } else {
            usage_error(prog, &format!("unrecognized arguments: {a}"));
        }
    }
    o
}
