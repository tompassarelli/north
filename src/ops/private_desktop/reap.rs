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
                "usage: north private-desktop reap [--dry-run]",
            ));
        }
    };
    let proc_root = Path::new("/proc");
    let live = live_sessions(proc_root);
    let recorded = crate::fleet::registry::recorded_here().map_err(io::Error::other)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn process(proc_root: &Path, pid: i32, comm: &str, session: Option<&str>) {
        let directory = proc_root.join(pid.to_string());
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("comm"), format!("{comm}\n")).unwrap();
        let environ = session.map_or_else(|| b"HOME=/home/tom\0".to_vec(), |name| format!("HOME=/home/tom\0NORTH_FLEET_SESSION={name}\0").into_bytes());
        fs::write(directory.join("environ"), environ).unwrap();
    }

    fn desktop(runtime: &Path, name: &str, launcher: i32) -> PathBuf {
        let run = runtime.join(name);
        fs::create_dir_all(&run).unwrap();
        fs::write(run.join("lifecycle-owned"), b"").unwrap();
        fs::write(run.join("launcher-pid"), format!("{launcher}\n")).unwrap();
        run
    }

    #[test]
    fn a_desktop_whose_recorded_fleet_session_cli_has_exited_is_an_orphan_and_others_are_kept() {
        let root = tempfile::tempdir().unwrap();
        let (proc_root, runtime) = (root.path().join("proc"), root.path().join("runtime"));
        process(&proc_root, 10, "claude", Some("sc-1"));
        process(&proc_root, 11, "north", Some("sc-1"));
        process(&proc_root, 20, "north", Some("sc-2"));
        process(&proc_root, 30, "north", None);
        process(&proc_root, 50, "north", Some("sc-away"));
        let kept = desktop(&runtime, "private-desktop.kept", 11);
        let orphan = desktop(&runtime, "private-desktop.orphan", 20);
        desktop(&runtime, "private-desktop.toms", 30);
        let away = desktop(&runtime, "private-desktop.away", 50);
        desktop(&runtime, "private-desktop.ended", 40);
        fs::create_dir_all(runtime.join("private-desktop.unowned")).unwrap();

        let live = live_sessions(&proc_root);
        assert_eq!(live, BTreeSet::from(["sc-1".to_owned()]));
        let mut found = desktops(&runtime, &proc_root).unwrap();
        found.sort_by(|left, right| left.run.cmp(&right.run));
        assert_eq!(found.iter().map(|desktop| &desktop.run).collect::<Vec<_>>(), [&away, &kept, &orphan]);
        let recorded = BTreeSet::from(["sc-1".to_owned(), "sc-2".to_owned()]);
        assert_eq!(orphans(found, &live, &recorded), [Desktop { run: orphan, launcher: 20, session: "sc-2".into() }]);
    }
}
