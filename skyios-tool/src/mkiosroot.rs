use crate::util::{die, which};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const USAGE: &str = "usage: skyios mkiosroot OS.dmg SystemOS.dmg AppOS.dmg OUT.img [SIZE]   (Linux, needs sudo)

Builds one case-sensitive journaled HFS+ iOS root from the IPSW's OS volume and both
cryptexes: hard-links the dyld cache and cryptex-only files into the system paths,
creates the log directories and a plain system keybag. SIZE defaults to 16G.";

const KEYBAG: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\"><dict><key>KeyBagKeys</key><data>U1dFRVQ=</data></dict></plist>
";

fn sh(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).status().map(|s| s.success()).unwrap_or(false)
}

fn must(cmd: &str, args: &[&str]) {
    if !sh(cmd, args) {
        die(format!("{cmd} {} failed", args.join(" ")));
    }
}

fn sudo(args: &[&str]) {
    must("sudo", args);
}

fn sudo_ok(args: &[&str]) -> bool {
    Command::new("sudo").args(args).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

fn sudo_lines(args: &[&str]) -> Vec<String> {
    let out = Command::new("sudo").args(args).output().unwrap_or_else(|e| die(e));
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

fn is_mounted(p: &Path) -> bool {
    sh("mountpoint", &["-q", &p.to_string_lossy()])
}

struct Work {
    dir: PathBuf,
    mounts: Vec<PathBuf>,
}

impl Drop for Work {
    fn drop(&mut self) {
        let mut busy = false;
        for m in &self.mounts {
            if is_mounted(m) {
                let _ = Command::new("sudo").arg("umount").arg(m).stderr(Stdio::null()).status();
            }
            busy |= is_mounted(m);
        }
        if busy {
            eprintln!("still mounted under {}: left in place", self.dir.display());
            return;
        }
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "dmg") {
                    let _ = std::fs::remove_file(&p);
                } else {
                    let _ = std::fs::remove_dir(&p);
                }
            }
        }
        let _ = std::fs::remove_dir(&self.dir);
    }
}

impl Work {
    fn apfs_mount(&mut self, image: &Path, name: &str) {
        let link = self.dir.join(format!("{name}.dmg"));
        std::os::unix::fs::symlink(image, &link).unwrap_or_else(|e| die(e));
        let mnt = self.dir.join(name);
        std::fs::create_dir(&mnt).unwrap_or_else(|e| die(e));
        let fuse = which("apfs-fuse").unwrap_or_else(|| die("apfs-fuse not found"));
        sudo(&[&fuse.to_string_lossy(), &link.to_string_lossy(), &mnt.to_string_lossy()]);
        self.mounts.push(mnt);
    }
}

fn copy_tree(src: &Path, dst: &Path, root: &Path) {
    let shown = dst.strip_prefix(root).map(|p| format!("/{}", p.display())).unwrap_or_else(|_| dst.display().to_string());
    println!("copying {} -> {}", src.display(), if shown == "/" { "".into() } else { shown });
    sudo(&["mkdir", "-p", &dst.to_string_lossy()]);
    sudo(&["rsync", "-aH", "--numeric-ids", &format!("{}/", src.display()), &format!("{}/", dst.display())]);
}

pub fn main(args: &[String]) -> i32 {
    if args.len() < 4 {
        eprintln!("{USAGE}");
        return 2;
    }
    let abs = |p: &str| std::fs::canonicalize(p).unwrap_or_else(|e| die(format!("{p}: {e}")));
    let (os, sysos, appos) = (abs(&args[0]), abs(&args[1]), abs(&args[2]));
    let out = args[3].as_str();
    let size = args.get(4).map(String::as_str).unwrap_or("16G");
    if Path::new(out).exists() {
        die(format!("refusing to overwrite {out}"));
    }
    let missing: Vec<&str> = ["apfs-fuse", "mkfs.hfsplus", "rsync"].into_iter().filter(|t| which(t).is_none()).collect();
    if !missing.is_empty() {
        eprintln!("Error: missing required host utilities: {}", missing.join(" "));
        eprintln!("  Install instructions:");
        eprintln!("    Arch Linux:    sudo pacman -S apfs-fuse hfsprogs rsync");
        eprintln!("    Ubuntu/Debian: sudo apt install apfs-fuse hfsprogs rsync");
        eprintln!("    Fedora:        sudo dnf install apfs-fuse hfsplus-tools rsync");
        return 1;
    }

    let tmp = Command::new("mktemp").arg("-d").output().unwrap_or_else(|e| die(e));
    let dir = PathBuf::from(String::from_utf8_lossy(&tmp.stdout).trim());
    let mut work = Work { dir: dir.clone(), mounts: Vec::new() };
    work.apfs_mount(&os, "os");
    work.apfs_mount(&sysos, "sysos");
    work.apfs_mount(&appos, "appos");

    must("truncate", &["-s", size, out]);
    must("mkfs.hfsplus", &["-s", "-J", "-v", "iOSRoot", out]);
    let root = dir.join("root");
    std::fs::create_dir(&root).unwrap_or_else(|e| die(e));
    sudo(&["mount", "-t", "hfsplus", "-o", "loop,force", out, &root.to_string_lossy()]);
    work.mounts.insert(0, root.clone());

    copy_tree(&dir.join("os/root"), &root, &root);
    copy_tree(&dir.join("sysos/root"), &root.join("private/preboot/Cryptexes/OS"), &root);
    copy_tree(&dir.join("appos/root"), &root.join("private/preboot/Cryptexes/App"), &root);

    let dsc_src = root.join("private/preboot/Cryptexes/OS/System/Library/Caches/com.apple.dyld");
    let dsc_dst = root.join("System/Library/Caches/com.apple.dyld");
    sudo(&["mkdir", "-p", &dsc_dst.to_string_lossy()]);
    for f in sudo_lines(&["find", &dsc_src.to_string_lossy(), "-mindepth", "1", "-maxdepth", "1"]) {
        let name = Path::new(&f).file_name().unwrap_or_default().to_string_lossy().into_owned();
        sudo(&["ln", &f, &dsc_dst.join(name).to_string_lossy()]);
    }

    for c in ["OS", "App"] {
        let base = root.join("private/preboot/Cryptexes").join(c);
        let files = sudo_lines(&[
            "find",
            &base.to_string_lossy(),
            "!",
            "-type",
            "d",
            "!",
            "-path",
            &format!("{}/System/Library/Caches/com.apple.dyld/*", base.display()),
        ]);
        for src in files {
            let rel = src.strip_prefix(&format!("{}/", base.display())).unwrap_or(&src).to_string();
            let dst = root.join(&rel);
            let d = dst.to_string_lossy();
            if sudo_ok(&["test", "-e", &d]) || sudo_ok(&["test", "-L", &d]) {
                continue;
            }
            sudo(&["mkdir", "-p", &dst.parent().unwrap_or(&root).to_string_lossy()]);
            if sudo_ok(&["test", "-L", &src]) {
                sudo(&["cp", "-P", &src, &d]);
            } else {
                sudo(&["ln", &src, &d]);
            }
            println!("linked {c}:/{rel}");
        }
    }

    let db = root.join("private/var/db");
    let dbs = |p: &str| db.join(p).to_string_lossy().into_owned();
    sudo(&["install", "-d", "-o", "272", "-g", "272", "-m", "0751", &dbs("diagnostics")]);
    sudo(&["install", "-d", "-o", "272", "-g", "272", "-m", "0755", &dbs("uuidtext"), &dbs("diagnostics/timesync")]);
    for d in ["diagnostics", "uuidtext", "diagnostics/timesync"] {
        sudo(&["install", "-o", "272", "-g", "272", "-m", "0644", "/dev/null", &dbs(&format!("{d}/.migrated"))]);
    }

    let keybags = root.join("private/var/keybags");
    sudo(&["install", "-d", "-m", "0700", "-o", "0", "-g", "0", &keybags.to_string_lossy()]);
    let kb = dir.join("systembag.kb");
    std::fs::write(&kb, KEYBAG).unwrap_or_else(|e| die(e));
    sudo(&["install", "-m", "0600", "-o", "0", "-g", "0", &kb.to_string_lossy(), &keybags.join("systembag.kb").to_string_lossy()]);
    let _ = std::fs::remove_file(&kb);

    sh("df", &["-h", &root.to_string_lossy()]);
    sudo(&["umount", &root.to_string_lossy()]);
    let fsck = Command::new("fsck.hfsplus").args(["-f", "-y", out]).output().unwrap_or_else(|e| die(e));
    for l in String::from_utf8_lossy(&fsck.stdout).lines().filter(|l| !l.contains("Illegal name")) {
        println!("{l}");
    }
    if sh("fsck.hfsplus", &["-f", "-n", out]) { 0 } else { 1 }
}
