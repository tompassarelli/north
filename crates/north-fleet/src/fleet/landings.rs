use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::NorthResult;
use crate::platform::process::run_within;

use super::books::Books;
use super::model::{Machine, SessionRecord};
use super::runtime::binary;

const REMOTE_LOG: &str = r#"cat "${XDG_STATE_HOME:-$HOME/.local/state}/safe-push/pushes.tsv""#;
const REMOTE_WAIT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Push {
    pub at_ms: u64,
    pub repository: String,
    pub branch: String,
    pub oid: String,
    pub landed: String,
    pub session: String,
}

pub fn observe_among(books: &dyn Books, record: &SessionRecord, pushes: &[Push]) -> NorthResult<Option<String>> {
    let Some(push) = landing(pushes, record) else { return Ok(None) };
    let task_ref = record.task_ref.as_deref().unwrap_or(&record.name);
    let recorded = books.record_landing(task_ref, &push.landed, push.at_ms, Some(push.at_ms.saturating_sub(record.created_ms)))?;
    Ok(recorded.then(|| push.landed.clone()))
}

pub fn landed_between(record: &SessionRecord, pushes: &[Push], from_ms: u64, to_ms: u64) -> Option<String> {
    landings_by(pushes, &record.name, from_ms, to_ms).next_back().map(|push| push.landed.clone())
}

/// The pushes safe-push logged for a session: this machine's log, plus, for a session whose CLI runs on another machine, that machine's log, where its landings are written.
pub struct PushLog {
    here: Vec<Push>,
    there: BTreeMap<String, Vec<Push>>,
}

impl PushLog {
    pub fn read(log: &Path) -> Self {
        Self { here: pushes(log), there: BTreeMap::new() }
    }

    pub fn of(&mut self, record: &SessionRecord) -> Vec<Push> {
        let remote = record.machine.as_ref().map(|machine| &*self.there.entry(machine.name.clone()).or_insert_with(|| remote_pushes(&binary("ssh"), machine)));
        self.here.iter().chain(remote.into_iter().flatten()).filter(|push| push.session == record.name).cloned().collect()
    }
}

pub fn pushes_of(record: &SessionRecord, push_log: &Path) -> Vec<Push> {
    PushLog::read(push_log).of(record)
}

pub fn remote_pushes(ssh: &Path, machine: &Machine) -> Vec<Push> {
    let argv = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", machine.ssh.as_str(), "--", REMOTE_LOG];
    match run_within(&ssh.to_string_lossy(), &argv, REMOTE_WAIT) {
        Some(output) if output.status.success() => parse(&String::from_utf8_lossy(&output.stdout)),
        Some(output) => {
            eprintln!("north ledger: pushes of {}: {}: {}", machine.name, output.status, String::from_utf8_lossy(&output.stderr).trim());
            Vec::new()
        }
        None => {
            eprintln!("north ledger: pushes of {}: no answer within {}s", machine.name, REMOTE_WAIT.as_secs());
            Vec::new()
        }
    }
}

pub fn landings_by<'a>(pushes: &'a [Push], session: &'a str, from_ms: u64, to_ms: u64) -> impl DoubleEndedIterator<Item = &'a Push> + 'a {
    pushes.iter().filter(move |push| push.session == session && !push.landed.is_empty() && push.at_ms + 1_000 > from_ms && push.at_ms <= to_ms)
}

pub fn push_log() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
    state.unwrap_or_else(home).join("safe-push/pushes.tsv")
}

fn landing<'a>(pushes: &'a [Push], record: &SessionRecord) -> Option<&'a Push> {
    pushes.iter().rev().find(|push| push.session == record.name && !push.landed.is_empty() && push.at_ms + 1_000 > record.created_ms)
}

pub fn pushes(log: &Path) -> Vec<Push> {
    parse(&std::fs::read_to_string(log).unwrap_or_default())
}

fn parse(text: &str) -> Vec<Push> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let at_secs: u64 = fields.first()?.parse().ok()?;
            let field = |index: usize| fields.get(index).copied().unwrap_or_default().to_owned();
            Some(Push { at_ms: at_secs * 1_000, repository: field(1), branch: field(2), oid: field(3), landed: field(4), session: field(5) })
        })
        .collect()
}

