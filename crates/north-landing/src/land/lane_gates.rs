use std::fs;
use std::path::{Path, PathBuf};

use super::scans::{policy, test_admission, test_integrity, tool_removal};
use crate::runtime_floor_gate::{self, Verdict};

pub struct Lane<'a> {
    pub worktree: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    pub author: Option<&'a str>,
}

pub struct Refused {
    pub verdict: String,
    pub log: PathBuf,
}

type Gate = fn(&Lane, &mut Vec<u8>) -> bool;

const GATES: &[(&str, Gate)] = &[
    ("runtime-floor gate", runtime_floor),
    ("test admission", admission),
    ("test integrity", integrity),
    ("tool removal", removal),
    ("policy sweep", sweep),
];

impl Lane<'_> {
    fn range(&self) -> String {
        format!("{}..{}", self.base, self.head)
    }
}

fn runtime_floor(lane: &Lane, out: &mut Vec<u8>) -> bool {
    let Ok(Verdict::Refused(lines)) = runtime_floor_gate::judge(lane.worktree, None, &runtime_floor_gate::state_dir(), lane.base, lane.head) else {
        return false;
    };
    for line in lines {
        out.extend_from_slice(format!("{line}\n").as_bytes());
    }
    out.extend_from_slice(b"safe-push: runtime-floor gate refused the landing; NOT pushed.\n");
    true
}

fn admission(lane: &Lane, out: &mut Vec<u8>) -> bool {
    test_admission::scan(lane.worktree, &lane.range(), lane.author, out) != 0
}

fn integrity(lane: &Lane, out: &mut Vec<u8>) -> bool {
    test_integrity::scan(lane.worktree, &lane.range(), out) != 0
}

fn removal(lane: &Lane, out: &mut Vec<u8>) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    tool_removal::scan(lane.worktree, &home, &lane.range(), out) != 0
}

fn sweep(lane: &Lane, out: &mut Vec<u8>) -> bool {
    policy::scan(lane.worktree, &lane.range(), out) != 0
}

pub fn judge(lane: &Lane, logs: &Path, name: &str) -> Result<Option<Refused>, String> {
    let mut text = Vec::new();
    let failed: Vec<&str> = GATES.iter().filter(|(_, gate)| gate(lane, &mut text)).map(|(label, _)| *label).collect();
    if failed.is_empty() {
        return Ok(None);
    }
    fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = logs.join(format!("{name}.lane-gates.log"));
    fs::write(&log, &text).map_err(|error| format!("{}: {error}", log.display()))?;
    Ok(Some(Refused { verdict: format!("failed {}; log: {}", failed.join(", "), log.display()), log }))
}
