use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rustix::process::{Pid, Signal, kill_process};

use super::run_state::{read_run_value, runtime_root};

const SESSION: &[u8] = b"NORTH_FLEET_SESSION=";
const SESSION_COMMS: [&str; 2] = ["claude", "codex"];

#[derive(Debug, PartialEq)]
pub(super) struct Desktop {
    pub(super) run: PathBuf,
    pub(super) launcher: i32,
    pub(super) session: String,
}

pub(super) fn run(arguments: &[String]) -> io::Result<u8> {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: north desktop private reap [--dry-run]",
            ));
        }
    };
    let proc_root = Path::new("/proc");
    let live = live_sessions(proc_root);
    let recorded = north_fleet_supervisor::fleet::registry::recorded_here().map_err(io::Error::other)?;
    for desktop in orphans(desktops(&runtime_root()?, proc_root)?, &live, &recorded) {
        let verb = if dry_run {
            "would stop"
        } else if Pid::from_raw(desktop.launcher).is_some_and(|pid| kill_process(pid, Signal::Term).is_ok()) {
            "stopped"
        } else {
            "could not stop"
        };
        println!(
            "{verb} private desktop {} (launcher {}): fleet session {} is not running",
            desktop.run.display(),
            desktop.launcher,
            desktop.session
        );
    }
    Ok(0)
}

pub(super) fn desktops(runtime: &Path, proc_root: &Path) -> io::Result<Vec<Desktop>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(runtime)? {
        let run = entry?.path();
        let named = run
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("private-desktop."));
        if !named || !run.join("lifecycle-owned").is_file() {
            continue;
        }
        let Some(launcher) = read_run_value(&run, "launcher-pid").ok().and_then(|pid| pid.parse().ok()) else {
            continue;
        };
        if let Some(session) = session_of(proc_root, launcher) {
            found.push(Desktop { run, launcher, session });
        }
    }
    Ok(found)
}

pub(super) fn live_sessions(proc_root: &Path) -> BTreeSet<String> {
    fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            fs::read_to_string(proc_root.join(pid.to_string()).join("comm"))
                .is_ok_and(|comm| SESSION_COMMS.contains(&comm.trim()))
        })
        .filter_map(|pid| session_of(proc_root, pid))
        .collect()
}

pub(super) fn orphans(desktops: Vec<Desktop>, live: &BTreeSet<String>, recorded: &BTreeSet<String>) -> Vec<Desktop> {
    desktops.into_iter().filter(|desktop| recorded.contains(&desktop.session) && !live.contains(&desktop.session)).collect()
}

fn session_of(proc_root: &Path, pid: i32) -> Option<String> {
    let environ = fs::read(proc_root.join(pid.to_string()).join("environ")).ok()?;
    environ
        .split(|byte| *byte == 0)
        .find_map(|variable| variable.strip_prefix(SESSION))
        .filter(|session| !session.is_empty())
        .map(|session| String::from_utf8_lossy(session).into_owned())
}
