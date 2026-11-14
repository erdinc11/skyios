use crate::launchd::{daemons, label, rename_panic_on_crash};
use crate::plistpy::{get, load, remove, set, to_binary, to_xml, V};
use crate::util::{die, py_list, write};
use std::fs;
use std::os::unix::fs::{chown, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

const BUILD: &str = "20H392";
const VERSION: &str = "16.7.16";
const MOBILE: u32 = 501;

const USAGE: &str = "Apply the SWEET bring-up changes to an iOS root made by tools/mkiosroot.sh.

  sudo sweet root-tweaks MOUNTED_ROOT";

fn s(v: &str) -> V {
    V::s(v)
}

fn dict(items: Vec<(&str, V)>) -> V {
    V::dict(items)
}

fn strs(v: &[&str]) -> V {
    V::strs(v)
}

fn chown_mod(path: &Path, uid: u32, gid: u32, mode: u32) {
    chown(path, Some(uid), Some(gid)).unwrap_or_else(|e| die(format!("chown {}: {e}", path.display())));
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap_or_else(|e| die(format!("chmod {}: {e}", path.display())));
}

fn save(path: &Path, data: &V, uid: u32, gid: u32, mode: u32, xml: bool) {
    write(path, &if xml { to_xml(data) } else { to_binary(data) });
    chown_mod(path, uid, gid, mode);
}

fn makedirs(d: &Path) {
    fs::create_dir_all(d).unwrap_or_else(|e| die(format!("mkdir {}: {e}", d.display())));
}

fn merge_pref(root: &Path, name: &str, values: V) {
    let path = root.join("private/var/mobile/Library/Preferences").join(format!("{name}.plist"));
    let data = if path.exists() { load(&path) } else { dict(vec![]) };
    let d = data.as_dict().unwrap_or_else(|| die(format!("{}: not a dictionary", path.display())));
    let values = values.as_dict().unwrap();
    let mut keys: Vec<String> = values.borrow().iter().map(|(k, _)| k.clone()).collect();
    keys.sort();
    for (k, v) in values.borrow().iter() {
        set(&d, k, v.clone());
    }
    save(&path, &data, MOBILE, MOBILE, 0o600, false);
    println!("pref {name} {}", py_list(&keys.iter().map(|k| k.as_str()).collect::<Vec<_>>()));
}

fn launchd(root: &Path) {
    let path = root.join("System/Library/xpc/launchd.plist");
    let st = fs::metadata(&path).unwrap_or_else(|e| die(format!("{}: {e}", path.display())));
    let plist = load(&path);
    let jobs = daemons(&plist);
    let mut renamed = 0;
    for (_, job) in &jobs {
        renamed += rename_panic_on_crash(job) as usize;
    }
    let removed: Vec<&String> =
        jobs.iter().filter(|(_, j)| label(j).as_deref() == Some("com.apple.CommCenter")).map(|(k, _)| k).collect();
    let all = plist.as_dict().and_then(|d| get(&d, "LaunchDaemons")).and_then(|v| v.as_dict()).unwrap();
    for k in &removed {
        remove(&all, k);
    }
    save(&path, &plist, st.uid(), st.gid(), st.mode() & 0o7777, false);
    println!("launchd.plist: {renamed} _PanicOnCrash renamed, {} CommCenter jobs removed", removed.len());
}

fn containers(root: &Path) {
    let base = root.join("private/var/containers");
    for (sub, uid) in [("Bundle", 33), ("Data/System", 0), ("Shared/SystemGroup", 0), ("Temp", 0)] {
        let d = base.join(sub);
        if !d.is_dir() {
            makedirs(&d);
            chown_mod(&d, uid, uid, 0o755);
            println!("mkdir {}", d.display());
        }
    }
}

fn glob_star(dir: &Path, leaf: &str) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path().join(leaf))
        .filter(|p| p.symlink_metadata().is_ok() || p.exists())
        .collect()
}

fn find_container(root: &Path, kind: &str, identifier: &str) -> Option<PathBuf> {
    for meta in glob_star(&root.join("private/var/containers").join(kind), ".com.apple.mobile_container_manager.metadata.plist") {
        let v = load(&meta);
        if v.as_dict().and_then(|d| get(&d, "MCMMetadataIdentifier")).and_then(|v| v.as_str().map(String::from)).as_deref()
            == Some(identifier)
        {
            return meta.parent().map(|p| p.to_path_buf());
        }
    }
    None
}

fn activation(root: &Path) {
    let Some(c) = find_container(root, "Data/System", "com.apple.mobileactivationd") else {
        println!("warning: no mobileactivationd system container in the image; skipped data ark");
        return;
    };
    let d = c.join("Library/internal");
    makedirs(&d);
    chown_mod(&d, MOBILE, MOBILE, 0o755);
    let ark = dict(vec![
        ("-ActivationState", s("Activated")),
        ("-BrickState", V::Bool(false)),
        ("-BuildVersion", s(BUILD)),
        ("-ProductVersion", s(VERSION)),
        ("-DataMigrationCompleted", V::Bool(true)),
        ("-PreSydroDataMigrationCompleted", V::Bool(true)),
    ]);
    save(&d.join("data_ark.plist"), &ark, MOBILE, MOBILE, 0o600, false);
    println!("data ark {}", d.display());
}

fn audio(root: &Path) {
    let src = root.join("Library/Audio/Plug-Ins/HAL/VirtualAudio.plugin");
    let dst_dir = root.join("Library/Audio/Plug-Ins/HAL.disabled");
    if src.exists() {
        makedirs(&dst_dir);
        let dst = dst_dir.join("VirtualAudio.plugin");
        if dst.exists() {
            die(format!("Destination path '{}' already exists", dst.display()));
        }
        fs::rename(&src, &dst).unwrap_or_else(|e| die(format!("move {}: {e}", src.display())));
        println!("VirtualAudio.plugin disabled");
    }
    for conf in glob_star(&root.join("System/Library/Frameworks/MediaToolbox.framework"), "SubServerConfiguration.plist") {
        let st = fs::metadata(&conf).unwrap_or_else(|e| die(format!("{}: {e}", conf.display())));
        let data = load(&conf);
        let d = data.as_dict().unwrap_or_else(|| die(format!("{}: not a dictionary", conf.display())));
        set(&d, "StartRecorderServer", V::Bool(false));
        save(&conf, &data, st.uid(), st.gid(), st.mode() & 0o7777, true);
        println!("StartRecorderServer off {}", conf.strip_prefix(root).unwrap_or(&conf).display());
    }
}

fn network(root: &Path) {
    let svc = "B2D7E5A4-0001-4E77-9EE0-5357EE7E0001";
    let nset = "B2D7E5A4-0002-4E77-9EE0-5357EE7E0002";
    let d = root.join("private/var/preferences/SystemConfiguration");
    makedirs(&d);
    let service = dict(vec![
        ("UserDefinedName", s("USB Ethernet")),
        (
            "Interface",
            dict(vec![
                ("DeviceName", s("ue0")),
                ("Hardware", s("Ethernet")),
                ("Type", s("Ethernet")),
                ("UserDefinedName", s("USB Ethernet")),
            ]),
        ),
        (
            "IPv4",
            dict(vec![
                ("ConfigMethod", s("Manual")),
                ("Addresses", strs(&["192.168.7.2"])),
                ("SubnetMasks", strs(&["255.255.255.0"])),
                ("Router", s("192.168.7.1")),
            ]),
        ),
        ("DNS", dict(vec![("ServerAddresses", strs(&["1.1.1.1", "8.8.8.8"]))])),
        (
            "Proxies",
            dict(vec![("ExceptionsList", strs(&["*.local", "169.254/16"])), ("FTPPassive", V::Int(1))]),
        ),
    ]);
    let prefs = dict(vec![
        ("CurrentSet", s(&format!("/Sets/{nset}"))),
        ("NetworkServices", dict(vec![(svc, service)])),
        (
            "Sets",
            dict(vec![(
                nset,
                dict(vec![
                    ("UserDefinedName", s("Automatic")),
                    (
                        "Network",
                        dict(vec![
                            ("Global", dict(vec![("IPv4", dict(vec![("ServiceOrder", strs(&[svc]))]))])),
                            ("Service", dict(vec![(svc, dict(vec![("__LINK__", s(&format!("/NetworkServices/{svc}")))]))])),
                        ]),
                    ),
                ]),
            )]),
        ),
        (
            "System",
            dict(vec![
                ("Network", dict(vec![("HostNames", dict(vec![("LocalHostName", s("sweet"))]))])),
                ("System", dict(vec![("ComputerName", s("sweet")), ("HostName", s("sweet"))])),
            ]),
        ),
    ]);
    save(&d.join("preferences.plist"), &prefs, 0, 0, 0o644, true);
    println!("network service USB Ethernet (ue0 192.168.7.2/24 via 192.168.7.1)");
}

pub fn main(args: &[String]) -> i32 {
    if args.len() != 1 || !Path::new(&args[0]).join("System/Library/xpc").is_dir() {
        die(USAGE);
    }
    let root = Path::new(&args[0]);
    launchd(root);
    containers(root);
    activation(root);
    audio(root);
    network(root);
    merge_pref(
        root,
        "com.apple.migration",
        dict(vec![
            ("LastSystemVersion", s(BUILD)),
            ("DMUserDataDisposition", dict(vec![("SyntheticDidUpgrade", V::Bool(false)), ("buildVersion", s(BUILD))])),
        ]),
    );
    merge_pref(
        root,
        "com.apple.springboard",
        dict(vec![
            ("BKNoWatchdogs", V::Bool(true)),
            ("FBLaunchWatchdogExceptions", dict(vec![("com.apple.purplebuddy", V::Real(100.0))])),
            ("MCForceShowCloudConfigurationUI", V::Bool(false)),
        ]),
    );
    merge_pref(
        root,
        "com.apple.purplebuddy",
        dict(vec![
            ("SetupDone", V::Bool(true)),
            ("SetupFinishedAllSteps", V::Bool(true)),
            ("SetupVersion", V::Int(11)),
            ("daemonizeNeedsToRun", V::Bool(false)),
        ]),
    );
    merge_pref(
        root,
        "com.apple.purplebuddy.notbackedup",
        dict(
            [
                "ShouldRunDiagnosticsMiniBuddy",
                "ShouldRunHSA2MiniBuddy",
                "ShouldRunKeychainSyncMiniBuddy",
                "ShouldRunMesaMiniBuddy",
                "ShouldRunPaymentMiniBuddy",
                "ShouldRunStandardMiniBuddy",
                "SilentICDPUpgrade",
            ]
            .iter()
            .map(|k| (*k, V::Bool(false)))
            .collect(),
        ),
    );
    merge_pref(
        root,
        "com.apple.managedconfiguration.notbackedup",
        dict(vec![("LockdownActivationIndicatesCloudConfigurationAvailable", V::Bool(false))]),
    );
    0
}
