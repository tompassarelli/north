use std::path::Path;
use std::process::{Command, Stdio};

use crate::land::after::follow::program;
use crate::land::telemetry;

pub struct Timeout<'a> {
    pub base: &'a str,
    pub candidate: &'a str,
    pub tree: &'a str,
    pub command: &'a str,
    pub deadline: u64,
    pub started: u64,
    pub finished: u64,
}

pub fn arguments(origin: &str, timeout: &Timeout) -> Vec<String> {
    let mut words = vec!["land-train".to_owned(), "check".to_owned()];
    for (flag, value) in [
        ("--repo", telemetry::repository(origin)),
        ("--candidate", timeout.candidate.to_owned()),
        ("--tree", timeout.tree.to_owned()),
        ("--base", timeout.base.to_owned()),
        ("--command", timeout.command.to_owned()),
        ("--deadline", timeout.deadline.to_string()),
        ("--started-at", timeout.started.to_string()),
        ("--finished-at", timeout.finished.to_string()),
        ("--outcome", "timeout".to_owned()),
    ] {
        words.extend([flag.to_owned(), value]);
    }
    words
}

pub fn record(root: &Path, timeout: &Timeout) {
    let origin = Command::new("git").args(["remote", "get-url", "origin"]).current_dir(root).stderr(Stdio::null()).output().ok().filter(|output| output.status.success());
    let Some(origin) = origin.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|origin| !origin.is_empty()) else { return };
    let spawned = Command::new(program()).args(arguments(&origin, timeout)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || child.wait());
    }
}
