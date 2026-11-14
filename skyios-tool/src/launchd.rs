use crate::plistpy::{get, load, remove, set, to_binary, Dict, V};
use crate::util::{die, write};
use std::collections::BTreeSet;

pub fn rename_panic_on_crash(job: &Dict) -> bool {
    match remove(job, "_PanicOnCrash") {
        Some(v) => {
            set(job, "_PanicOnCrasX", v);
            true
        }
        None => false,
    }
}

pub fn daemons(plist: &V) -> Vec<(String, Dict)> {
    let d = plist
        .as_dict()
        .and_then(|d| get(&d, "LaunchDaemons"))
        .and_then(|v| v.as_dict())
        .unwrap_or_else(|| die("no LaunchDaemons dictionary"));
    let jobs = d.borrow().iter().filter_map(|(k, v)| v.as_dict().map(|j| (k.clone(), j))).collect();
    jobs
}

pub fn label(job: &Dict) -> Option<String> {
    get(job, "Label").and_then(|v| v.as_str().map(String::from))
}

pub fn main(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: skyios patch-launchd-plist IN.plist OUT.plist LABEL [LABEL ...]");
        return 2;
    }
    let (src, dst) = (&args[0], &args[1]);
    let labels: BTreeSet<&str> = args[2..].iter().map(|s| s.as_str()).collect();
    let plist = load(src);
    let mut found = BTreeSet::new();
    for (_, job) in daemons(&plist) {
        rename_panic_on_crash(&job);
        let Some(l) = label(&job).filter(|l| labels.contains(l.as_str())) else { continue };
        set(&job, "StandardOutPath", V::s("/dev/console"));
        set(&job, "StandardErrorPath", V::s("/dev/console"));
        if get(&job, "EnvironmentVariables").is_none() {
            set(&job, "EnvironmentVariables", V::dict(vec![]));
        }
        let env = get(&job, "EnvironmentVariables")
            .and_then(|v| v.as_dict())
            .unwrap_or_else(|| die("EnvironmentVariables is not a dictionary"));
        set(&env, "OS_ACTIVITY_DT_MODE", V::s("YES"));
        set(&env, "CFLOG_FORCE_STDERR", V::s("1"));
        found.insert(l);
    }
    let missing: Vec<&str> = labels.iter().filter(|l| !found.contains(**l)).copied().collect();
    if !missing.is_empty() {
        die(format!("jobs not found: {}", missing.join(", ")));
    }
    write(dst, &to_binary(&plist));
    println!("{dst}: console logging for {}", found.into_iter().collect::<Vec<_>>().join(", "));
    0
}
