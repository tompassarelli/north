use std::fs;
use std::path::{Path, PathBuf};

use super::scans::{incident_guard, policy, test_admission, test_integrity, tool_removal};
use crate::runtime_floor_gate::{self, Verdict};

pub struct Lane<'a> {
    pub worktree: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    pub state_home: &'a Path,
}

pub struct Refused {
    pub verdict: String,
    pub log: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Judging {
    Live,
    Recorded,
}

type Gate = fn(&Lane, &mut Vec<u8>) -> bool;

const READS_LIVE_STATE: [&str; 1] = ["runtime-floor gate"];

const GATES: &[(&str, Gate)] = &[
    ("runtime-floor gate", runtime_floor),
    ("test admission", admission),
    ("test integrity", integrity),
    ("incident test", incident),
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
    test_admission::scan(lane.worktree, &lane.range(), lane.state_home, out) != 0
}

fn integrity(lane: &Lane, out: &mut Vec<u8>) -> bool {
    test_integrity::scan(lane.worktree, &lane.range(), out) != 0
}

fn incident(lane: &Lane, out: &mut Vec<u8>) -> bool {
    incident_guard::scan(lane.worktree, &lane.range(), out) != 0
}

fn removal(lane: &Lane, out: &mut Vec<u8>) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    tool_removal::scan(lane.worktree, &home, &lane.range(), out) != 0
}

fn sweep(lane: &Lane, out: &mut Vec<u8>) -> bool {
    policy::scan(lane.worktree, &lane.range(), out) != 0
}

pub fn failed(lane: &Lane, judging: Judging) -> Option<(Vec<&'static str>, Vec<u8>)> {
    let mut text = Vec::new();
    let judged = GATES.iter().filter(|(label, _)| judging == Judging::Live || !READS_LIVE_STATE.contains(label));
    let failed: Vec<&str> = judged.filter(|(_, gate)| gate(lane, &mut text)).map(|(label, _)| *label).collect();
    (!failed.is_empty()).then_some((failed, text))
}

pub fn judge(lane: &Lane, judging: Judging, logs: &Path, name: &str) -> Result<Option<Refused>, String> {
    let Some((failed, text)) = failed(lane, judging) else { return Ok(None) };
    fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = logs.join(format!("{name}.lane-gates.log"));
    fs::write(&log, &text).map_err(|error| format!("{}: {error}", log.display()))?;
    Ok(Some(Refused { verdict: format!("failed {}; log: {}", failed.join(", "), log.display()), log }))
}
