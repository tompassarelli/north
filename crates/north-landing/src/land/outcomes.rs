use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::queue;
use crate::land_model::features::{Columns, Record};
use crate::land_model::Class;

pub const FILE: &str = "outcomes.jsonl";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Green,
    Refused,
    Infra,
    RedMain,
    Killed,
    Moved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Precheck,
    Group,
    Find,
    Prefix,
    PostLand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Landed,
    Conflict,
    Failed,
    Infra,
    Replaced,
    Reverted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Fast,
    Precheck,
    Group,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cells {
    pub conflict: String,
    pub pass: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    pub ticket: String,
    pub branch: String,
    pub head: String,
    pub at: i64,
    pub cells: Cells,
    pub columns: Columns,
    pub neighbours: Vec<String>,
    pub policy: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub candidate: String,
    pub lanes: Vec<String>,
    pub host: String,
    pub lease_cpus: u32,
    pub queued_secs: f64,
    pub run_secs: f64,
    pub checks: u32,
    pub verdict: Verdict,
    pub phase: Phase,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decided {
    pub ticket: String,
    pub class: Outcome,
    pub route: Route,
    pub batch_size: u32,
    pub policy: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deferred {
    pub ticket: String,
    pub behind: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Line {
    Seen(Seen),
    Check(Check),
    Decided(Decided),
    Deferred(Deferred),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ledger {
    pub lines: Vec<Line>,
    pub skipped: usize,
}

pub fn path(queue: &Path) -> PathBuf {
    queue.join(FILE)
}

pub fn ticket_name(id: u128, branch: &str) -> String {
    format!("{id}-{}", queue::slug(branch))
}

pub fn append(queue: &Path, line: &Line) -> io::Result<()> {
    fs::create_dir_all(queue)?;
    let mut text = serde_json::to_string(line).map_err(io::Error::other)?;
    text.push('\n');
    OpenOptions::new().create(true).append(true).open(path(queue))?.write_all(text.as_bytes())
}

pub fn read(queue: &Path) -> io::Result<Ledger> {
    let text = match fs::read_to_string(path(queue)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Ledger::default()),
        Err(error) => return Err(error),
    };
    let mut ledger = Ledger::default();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str(line) {
            Ok(parsed) => ledger.lines.push(parsed),
            Err(_) => ledger.skipped += 1,
        }
    }
    Ok(ledger)
}

pub fn join(records: Vec<Record>, ledger: &Ledger) -> Vec<Record> {
    let reverted: BTreeMap<&str, ()> = ledger
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::Decided(decided) if decided.class == Outcome::Reverted => Some((decided.ticket.as_str(), ())),
            _ => None,
        })
        .collect();
    records
        .into_iter()
        .map(|record| {
            let reverted = reverted.contains_key(ticket_name(record.id, &record.branch).as_str());
            Record { class: if reverted && record.class == Class::Landed { Class::Failed } else { record.class }, ..record }
        })
        .collect()
}

#[cfg(test)]
#[path = "outcomes_tests.rs"]
mod tests;
