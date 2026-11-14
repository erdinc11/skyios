use crate::util::die;
use regex::{NoExpand, Regex};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

const CI_DXB: &str = "/Users/runner/work/ios-sweet/ios-sweet/dxb";
const DROP: [&str; 4] = ["-c", "-g", "-MD", "-MMD"];
const DROP_WITH_ARG: [&str; 4] = ["-MF", "-MT", "-o", "-arch"];
const COMPONENTS: [&str; 8] = ["osfmk", "bsd", "iokit", "libkern", "pexpert", "security", "san", "libsa"];

type Cmd = (String, Vec<String>);

#[derive(Default)]
struct Cmds {
    keys: Vec<String>,
    map: HashMap<String, Cmd>,
}

impl Cmds {
    fn insert(&mut self, k: String, v: Cmd) {
        if !self.map.contains_key(&k) {
            self.keys.push(k.clone());
        }
        self.map.insert(k, v);
    }
    fn iter(&self) -> impl Iterator<Item = (&String, &Cmd)> {
        self.keys.iter().map(|k| (k, &self.map[k]))
    }
}

fn shlex(s: &str) -> Option<Vec<String>> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut have = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            c if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            '\'' => {
                have = true;
                loop {
                    match it.next()? {
                        '\'' => break,
                        c => cur.push(c),
                    }
                }
            }
            '"' => {
                have = true;
                loop {
                    match it.next()? {
                        '"' => break,
                        '\\' => match it.next()? {
                            c @ ('\\' | '"') => cur.push(c),
                            c => {
                                cur.push('\\');
                                cur.push(c)
                            }
                        },
                        c => cur.push(c),
                    }
                }
            }
            '\\' => {
                have = true;
                cur.push(it.next()?);
            }
            c => {
                have = true;
                cur.push(c)
            }
        }
    }
    if have {
        out.push(cur);
    }
    Some(out)
}

fn ext(p: &str) -> &str {
    let base = p.rsplit('/').next().unwrap();
    match base.rfind('.') {
        Some(i) if base[..i].chars().any(|c| c != '.') => &base[i..],
        _ => "",
    }
}

fn commands(log: &str) -> Cmds {
    let bytes = std::fs::read(log).unwrap_or_else(|e| die(format!("{log}: {e}")));
    let text = String::from_utf8_lossy(&bytes);
    let prefix = Regex::new(r"^build\s+Build\s+\S+\s+").unwrap();
    let clang = Regex::new(r"(\S+/clang(\+\+)?) (.*)").unwrap();
    let src_re = Regex::new(r"\.(c|cpp|s|S)$").unwrap();
    let mut seen = Cmds::default();
    for line in text.split_inclusive('\n') {
        if !line.contains("/clang") || !line.contains(" -c ") {
            continue;
        }
        let line = prefix.replace(line.trim_end_matches('\n'), "").into_owned();
        let Some(m) = clang.captures(&line) else { continue };
        let Some(args) = shlex(&m[3]) else { continue };
        let srcs: Vec<&String> = args.iter().filter(|a| src_re.is_match(a) && a.starts_with(CI_DXB)).collect();
        if srcs.len() == 1 && srcs[0].contains("/xnu/") {
            let kind = if m.get(2).is_some() { "clang++" } else { "clang" };
            seen.insert(srcs[0].clone(), (kind.into(), args.clone()));
        }
    }
    seen
}

fn walk(dir: &Path, f: &mut dyn FnMut(&str, &[String])) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let (mut dirs, mut files) = (vec![], vec![]);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let ft = e.file_type().ok();
        let is_link = ft.map(|t| t.is_symlink()).unwrap_or(false);
        let is_dir = if is_link { e.path().is_dir() } else { ft.map(|t| t.is_dir()).unwrap_or(false) };
        if is_dir {
            if !is_link {
                dirs.push(e.path());
            }
        } else {
            files.push(name);
        }
    }
    f(&dir.to_string_lossy(), &files);
    for d in dirs {
        walk(&d, f);
    }
}

fn makefile_sources(bt: &str) -> Vec<String> {
    let mut out = vec![];
    walk(&Path::new(bt).join("build/xnu.obj"), &mut |root, files| {
        if !files.iter().any(|f| f == "Makefile") || !files.iter().any(|f| f == "meta_features.h") {
            return;
        }
        let raw = std::fs::read(Path::new(root).join("Makefile")).unwrap_or_default();
        let text = String::from_utf8_lossy(&raw).replace("\\\n", " ");
        for var in ["CFILES", "CXXFILES", "SFILES"] {
            let re = Regex::new(&format!(r"(?m)^{var}=(.*)$")).unwrap();
            let Some(m) = re.captures(&text) else { continue };
            for f in m[1].split_whitespace() {
                let mut f = f.replace("$(SOURCE_DIR)", &format!("{CI_DXB}/xnu"));
                if let Some(rest) = f.strip_prefix("./") {
                    f = format!("{}/{rest}", root.replace(bt, CI_DXB));
                }
                out.push(f);
            }
        }
    });
    out
}

fn template_for(src: &str, cmds: &Cmds) -> Option<Cmd> {
    let comp = COMPONENTS.iter().find(|c| src.contains(&format!("/{c}/"))).copied().unwrap_or("None");
    let lang = ext(src);
    for (other, (kind, args)) in cmds.iter() {
        if ext(other) == lang && other.contains(&format!("/xnu/{comp}/")) {
            return Some((kind.clone(), args.iter().map(|a| if a == other { src.to_string() } else { a.clone() }).collect()));
        }
    }
    None
}

fn component_dirs(bt: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    walk(&Path::new(bt).join("build/xnu.obj"), &mut |root, files| {
        if files.iter().any(|f| f == "meta_features.h") {
            for c in COMPONENTS {
                if format!("{root}/").contains(&format!("/{c}/")) && !out.contains_key(c) {
                    out.insert(c.to_string(), root.to_string());
                }
            }
        }
    });
    out
}

fn env_words(k: &str) -> Vec<String> {
    std::env::var(k).unwrap_or_default().split_whitespace().map(String::from).collect()
}

fn run(src: &str, kind: &str, args: &[String], bt: &str, cdirs: &HashMap<String, String>) -> (i32, String) {
    let home = std::env::var("HOME").unwrap_or_default();
    let clang = format!("{home}/swift/usr/bin/clang");
    let sdk = Regex::new(r"/Applications/Xcode[^ ]*?\.sdk/+System/Library/Frameworks/Kernel\.framework/Versions/A/PrivateHeaders/platform").unwrap();
    let mode: Vec<String> = match std::env::var("XNU_CC_OBJDIR") {
        Ok(d) if !d.is_empty() => {
            let obj = src.replace(&format!("{CI_DXB}/xnu/"), "").replace('/', "__") + ".o";
            vec!["-c".into(), "-o".into(), Path::new(&d).join(obj).to_string_lossy().into_owned()]
        }
        _ => vec!["-fsyntax-only".into()],
    };
    let mut new: Vec<String> = vec![
        if kind == "clang" { clang.clone() } else { clang + "++" },
        "-target".into(),
        "arm64-apple-macos14.0".into(),
    ];
    new.extend(mode);
    new.extend(
        [
            "-D__builtin_xnu_type_signature(...)=\"\"",
            "-D__builtin_xnu_type_summary(...)=0",
            "-D__builtin_xnu_types_compatible(...)=1",
        ]
        .map(String::from),
    );
    new.extend(env_words("XNU_CC_EXTRA"));
    let platform = Path::new(bt).join("sdk-kernel-platform/platform").to_string_lossy().into_owned();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if DROP.contains(&a.as_str()) {
            continue;
        }
        if DROP_WITH_ARG.contains(&a.as_str()) {
            skip = true;
            continue;
        }
        new.push(sdk.replace_all(a, NoExpand(&platform)).replace(CI_DXB, bt));
    }
    new.extend(env_words("XNU_CC_POST"));
    let comp = COMPONENTS
        .iter()
        .find(|c| src.contains(&format!("/xnu/{c}/")))
        .or_else(|| COMPONENTS.iter().find(|c| src.contains(&format!("/{c}/"))));
    let Some(cwd) = comp.and_then(|c| cdirs.get(*c)) else {
        return (1, format!("no build dir for component {}", comp.copied().unwrap_or("None")));
    };
    match Command::new(&new[0]).args(&new[1..]).current_dir(cwd).output() {
        Ok(o) => (o.status.code().unwrap_or(1), String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(e) => (1, format!("{}: {e}", new[0])),
    }
}

fn splitlines(s: &str) -> Vec<&str> {
    s.split('\n').flat_map(|l| l.split('\r')).filter(|l| !l.is_empty()).collect()
}

fn strip_bt(l: &str, bt: &str) -> String {
    l.replace(&format!("{bt}/xnu/"), "").replace(&format!("{bt}/"), "")
}

pub fn main(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: skyios xnu-cc LOG BUILDTREE [FILTER ...] [--per-file]");
        return 2;
    }
    let log = &args[0];
    let bt = std::path::absolute(&args[1]).unwrap_or_else(|e| die(e)).to_string_lossy().into_owned();
    let per_file = args.iter().any(|a| a == "--per-file");
    let filters: Vec<&String> = args[2..].iter().filter(|a| *a != "--per-file").collect();
    let mut cmds = commands(log);
    let mut synth = 0;
    for src in makefile_sources(&bt) {
        if !cmds.map.contains_key(&src) {
            if let Some(t) = template_for(&src, &cmds) {
                cmds.insert(src, t);
                synth += 1;
            }
        }
    }
    for f in &filters {
        let is_src = [".c", ".cpp", ".s", ".S"].iter().any(|e| f.ends_with(e));
        if Path::new(&bt).join("xnu").join(f).is_file() && is_src {
            let src = format!("{CI_DXB}/xnu/{f}");
            if !cmds.map.contains_key(&src) {
                if let Some(t) = template_for(&src, &cmds) {
                    cmds.insert(src, t);
                    synth += 1;
                }
            }
        }
    }
    eprintln!("{synth} commands synthesized from Makefile source lists");
    let items: Vec<(String, Cmd)> = cmds
        .iter()
        .filter(|(k, _)| filters.is_empty() || filters.iter().any(|f| k.contains(f.as_str())))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if items.is_empty() {
        eprintln!("no compile commands matched");
        return 1;
    }
    let cdirs = component_dirs(&bt);
    let mut names: Vec<&String> = cdirs.keys().collect();
    names.sort();
    eprintln!(
        "{} files, build dirs: [{}]",
        items.len(),
        names.iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(", ")
    );
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut failed = vec![];
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    let nthreads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    std::thread::scope(|s| {
        for _ in 0..nthreads {
            let tx = tx.clone();
            let (items, next, bt, cdirs) = (&items, &next, &bt, &cdirs);
            s.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((src, (kind, a))) = items.get(i) else { break };
                let r = run(src, kind, a, bt, cdirs);
                if tx.send((i, r)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        let mut pending = BTreeMap::new();
        let mut want = 0;
        for (i, r) in rx {
            pending.insert(i, r);
            while let Some((rc, err)) = pending.remove(&want) {
                let src = &items[want].0;
                want += 1;
                if rc != 0 {
                    failed.push(src.replace(&format!("{CI_DXB}/xnu/"), ""));
                    if per_file {
                        println!("{}", failed.last().unwrap());
                        for l in splitlines(&err).into_iter().filter(|l| l.contains("error:")).take(2) {
                            println!("    {}", strip_bt(l, &bt).chars().take(220).collect::<String>());
                        }
                    }
                }
                for l in splitlines(&err) {
                    if l.contains(" error: ") || l.contains("fatal error:") {
                        *errors.entry(strip_bt(l, &bt)).or_default() += 1;
                    }
                }
            }
        }
    });
    if !per_file {
        for (e, n) in &errors {
            println!("{n:3} {e}");
        }
    }
    eprintln!("\n{} of {} files failed", failed.len(), items.len());
    (!failed.is_empty()) as i32
}
