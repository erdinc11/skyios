use crate::util::{parse, repo_root};
use regex::Regex;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

fn green(t: &str) -> String {
    format!("\x1b[92m{t}\x1b[0m")
}

fn red(t: &str) -> String {
    format!("\x1b[91m{t}\x1b[0m")
}

fn bold(t: &str) -> String {
    format!("\x1b[1m{t}\x1b[0m")
}

fn listing(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let n = p.file_name().unwrap().to_string_lossy().into_owned();
                    !n.starts_with('.') && n.ends_with(ext)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn report(errors: &[String], what: &str) -> bool {
    println!("{}", red(&format!("[-] {what} validation failed with {} error(s):", errors.len())));
    for e in errors {
        println!("    - {e}");
    }
    false
}

fn validate_patches(root: &Path) -> bool {
    let files = listing(&root.join("xnu/patches"), ".patch");
    if files.is_empty() {
        println!("{}", red("[-] Error: No patch files found in xnu/patches/"));
        return false;
    }
    let re = Regex::new(r"^(\d{4})-(.+)\.patch$").unwrap();
    let mut errors = vec![];
    let mut seen: BTreeMap<u32, String> = BTreeMap::new();
    for path in &files {
        let fname = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(m) = re.captures(&fname) else {
            errors.push(format!("Invalid patch filename format: {fname}"));
            continue;
        };
        let num: u32 = m[1].parse().unwrap();
        if let Some(prev) = seen.get(&num) {
            errors.push(format!("Duplicate patch number {num:04}: {fname} and {prev}"));
        }
        seen.insert(num, fname.clone());
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) == 0 {
            errors.push(format!("Patch is empty: {fname}"));
            continue;
        }
        let mut head = vec![0u8; 2048];
        let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
        let head = String::from_utf8_lossy(&head[..n]);
        if !(head.contains("--- ") || head.contains("diff ") || head.contains("Index:")) {
            errors.push(format!("Patch missing standard diff header: {fname}"));
        }
    }
    let mut expected = 1;
    for (num, fname) in &seen {
        if *num != expected {
            errors.push(format!("Gap in patch sequence: expected {expected:04}, found {num:04} ({fname})"));
        }
        expected = num + 1;
    }
    if !errors.is_empty() {
        return report(&errors, "Patch");
    }
    println!(
        "{}",
        green(&format!("[✓] Validated {} patches (0001-{:04} contiguous, format OK)", files.len(), files.len()))
    );
    true
}

fn validate_shell_scripts(root: &Path, verbose: bool) -> bool {
    let mut files = listing(&root.join("tools"), ".sh");
    files.extend(listing(&root.join("shim"), ".sh"));
    files.extend(listing(&root.join("xnu/patches"), ".sh"));
    files.sort();
    let mut errors = vec![];
    for path in &files {
        let rel = path.strip_prefix(root).unwrap_or(path).display().to_string();
        match Command::new("bash").arg("-n").arg(path).output() {
            Ok(o) if o.status.success() => {
                if verbose {
                    println!("    [OK] {rel}");
                }
            }
            Ok(o) => errors.push(format!("{rel}: {}", String::from_utf8_lossy(&o.stderr).trim())),
            Err(e) => errors.push(format!("{rel}: {e}")),
        }
    }
    if !errors.is_empty() {
        return report(&errors, "Shell syntax");
    }
    println!("{}", green(&format!("[✓] Validated {} Shell scripts (syntax OK)", files.len())));
    true
}

pub fn main(args: &[String]) -> i32 {
    let mut args = args.to_vec();
    for a in args.iter_mut() {
        if a == "-v" {
            *a = "--verbose".into();
        }
    }
    let o = parse("validate-repo", &args, &[], &["--verbose"]);
    let root = repo_root();
    println!("{}", bold("=== Running SkyIOS Repository Validation ==="));
    let ok_patches = validate_patches(&root);
    let ok_shell = validate_shell_scripts(&root, o.flag("--verbose"));
    if ok_patches && ok_shell {
        println!("{}", bold(&green("\nAll repository integrity checks PASSED successfully!")));
        0
    } else {
        println!("{}", bold(&red("\nRepository integrity check FAILED.")));
        1
    }
}
