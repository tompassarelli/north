use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::after::gates::{self, Landed};
use super::scans::{git, incident_guard, policy, push_jobs, test_admission, test_integrity, tool_removal};
use super::slots::select::Config;
use crate::advisory_gate;
use crate::dep_gate::{self, Registry};

pub struct Lane<'a> {
    pub worktree: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    pub state_home: &'a Path,
    pub code_health: &'a Path,
    pub registry: &'a dyn Registry,
    pub advisory_db: &'a Path,
}

#[derive(Clone)]
pub struct Refused {
    pub gates: String,
    pub verdict: String,
    pub log: PathBuf,
}

type Gate = fn(&Lane, &mut Vec<u8>) -> bool;

const GATES: &[(&str, Gate)] = &[
    ("test admission", admission),
    ("test integrity", integrity),
    ("incident test", incident),
    ("tool removal", removal),
    ("policy sweep", sweep),
    ("after-only reason", after_only),
    ("dependency age", dependency_age),
    ("dependency advisory", dependency_advisory),
    ("guard suite", guard_suite),
];

impl Lane<'_> {
    fn range(&self) -> String {
        format!("{}..{}", self.base, self.head)
    }
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

fn after_only(lane: &Lane, out: &mut Vec<u8>) -> bool {
    let config = Config::load(lane.worktree, lane.head);
    let unreasoned = config.after_without_reason();
    for command in &unreasoned {
        let _ = writeln!(out, "after-only reason: .safe-push runs `{command}` only after landing; list it as a land.check so it runs before, or record why it cannot with `land.after-only = REASON :: {command}`");
    }
    let unnamed = push_jobs_without_check(lane, &config, out);
    !unreasoned.is_empty() || unnamed
}

fn push_jobs_without_check(lane: &Lane, config: &Config, out: &mut Vec<u8>) -> bool {
    let jobs = match push_jobs::newly_on_main(lane.worktree, lane.base, lane.head) {
        Ok(jobs) => jobs,
        Err(error) => {
            let _ = writeln!(out, "after-only reason: cannot read the workflows this lane adds to a push to main: {error}");
            return true;
        }
    };
    let unnamed: Vec<_> = jobs.iter().filter(|job| !job.runs_only(|command| config.runs(command)) && config.after_only_reason(&job.name).is_none()).collect();
    for job in &unnamed {
        let _ = writeln!(out, "after-only reason: this lane adds or changes the GitHub Actions job `{}`, which runs on a push to main only after landing; make each of its run steps a land.check so it runs before, or record why it cannot with `land.after-only = REASON :: {}`", job.name, job.name);
    }
    !unnamed.is_empty()
}

fn dependency_age(lane: &Lane, out: &mut Vec<u8>) -> bool {
    dep_gate::refuse_young(lane.worktree, lane.base, lane.head, lane.registry, out)
}

fn dependency_advisory(lane: &Lane, out: &mut Vec<u8>) -> bool {
    advisory_gate::refuse(lane.worktree, lane.base, lane.head, lane.advisory_db, out)
}

fn guard_suite(lane: &Lane, out: &mut Vec<u8>) -> bool {
    super::slots::guard_suites::refuse(lane.worktree, lane.base, lane.head, out)
}

pub fn failed(lane: &Lane) -> Option<(Vec<&'static str>, Vec<u8>)> {
    let mut text = Vec::new();
    let mut failed: Vec<&str> = GATES.iter().filter(|(_, gate)| gate(lane, &mut text)).map(|(label, _)| *label).collect();
    failed.extend(gates::before(&Landed { tree: lane.worktree, base: lane.base, head: lane.head, code_health: lane.code_health }, &mut text));
    (!failed.is_empty()).then_some((failed, text))
}

pub fn judged_before_queueing(worktree: &Path, judged_from: Option<&str>, lane_head: &str, picked_onto: &str) -> bool {
    judged_from == Some(picked_onto) && git::text(worktree, &["rev-parse", "HEAD^{tree}", &format!("{lane_head}^{{tree}}")]).is_some_and(|trees| trees.lines().collect::<HashSet<_>>().len() == 1)
}

pub fn judge(lane: &Lane, logs: &Path, name: &str) -> Result<Option<Refused>, String> {
    let Some((failed, text)) = failed(lane) else { return Ok(None) };
    fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = logs.join(format!("{name}.lane-gates.log"));
    fs::write(&log, &text).map_err(|error| format!("{}: {error}", log.display()))?;
    let gates = failed.join(", ");
    Ok(Some(Refused { verdict: format!("failed {gates}; log: {}", log.display()), gates, log }))
}

#[cfg(test)]
#[path = "lane_gates_tests.rs"]
mod tests;
