//! Sessions shown as working that have written nothing past the limit of the call they wait on: a hung call leaves the row `working` with no one the wiser until a person or the lead notices.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use super::model::{Delivery, EntryKind, SessionRecord, Status};
use super::registry::Fleet;
use super::runtime::summarize;
use super::sender::Sender;
use super::transcript::last_written;

const FILE: &str = ".silent-told.json";
const HAND_IN: Duration = Duration::from_secs(31 * 60);
const CALL: Duration = Duration::from_secs(11 * 60);
const LAST_CHARS: usize = 300;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Silent {
    pub name: String,
    pub host: String,
    pub parent: Option<String>,
    pub last: String,
    pub quiet: Duration,
    pub written_ms: u64,
}

impl Silent {
    pub fn line(&self) -> String {
        format!("{} ({}) working with nothing written for {}m; last record: {}", self.name, self.host, self.quiet.as_secs() / 60, self.last)
    }
}

/// How long the call a session waits on may run quietly: a foreground hand-in waits up to 30 minutes, and any other call ends at the 10-minute tool limit; both carry a minute's margin.
pub fn call_limit(last: Option<(EntryKind, &str)>) -> Duration {
    match last {
        Some((EntryKind::Tool, text)) if text.starts_with("mcp__north__") || text.contains("safe-push ") => HAND_IN,
        _ => CALL,
    }
}

impl Fleet {
    pub fn silent_sessions(&self, now: SystemTime) -> Vec<Silent> {
        let records = self.sessions().unwrap_or_default();
        records.iter().filter_map(|record| self.silent(record, now)).collect()
    }

    fn silent(&self, record: &SessionRecord, now: SystemTime) -> Option<Silent> {
        if record.adopted() || self.status(record) != Status::Working || !self.running(record) {
            return None;
        }
        let written = last_written(record)?;
        let quiet = now.duration_since(written).ok()?;
        let follower = self.follow_recent(&record.name).ok()?;
        let entries = &follower.transcript().entries;
        let last = entries.last();
        (quiet > call_limit(last.map(|entry| (entry.kind, entry.text.as_str())))).then(|| Silent {
            name: record.name.clone(),
            host: self.host().to_owned(),
            parent: record.parent.clone(),
            last: last.map_or_else(|| "none".to_owned(), |entry| format!("{} {}", entry.kind.label(), summarize(&entry.text.replace('\n', " "), LAST_CHARS))),
            quiet,
            written_ms: written.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX)),
        })
    }

    /// Tells each silent session's parent once per silence, naming the session and its last record; the book keeps the write time it told of, so a session that wrote and went silent again is told of again.
    pub fn tell_silent(&self, now: SystemTime) -> Vec<String> {
        let file = self.root.join(FILE);
        let mut told: BTreeMap<String, u64> = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        let silent = self.silent_sessions(now);
        told.retain(|name, _| silent.iter().any(|one| &one.name == name));
        let mut lines = Vec::new();
        for one in &silent {
            let Some(parent) = &one.parent else { continue };
            if told.get(&one.name) == Some(&one.written_ms) {
                continue;
            }
            let text = format!("[north: {}; stalled past its running call's limit; restaff or stop it]", one.line());
            match self.send_signed(&Sender::fleet(), parent, &text, Delivery::Queued) {
                Ok(()) => {
                    told.insert(one.name.clone(), one.written_ms);
                    lines.push(format!("told {parent} that {} is silent", one.name));
                }
                Err(error) => lines.push(format!("{parent} was not told that {} is silent: {}", one.name, error.user_message())),
            }
        }
        if let Err(error) = serde_json::to_vec(&told).map_err(std::io::Error::from).and_then(|bytes| std::fs::write(&file, bytes)) {
            lines.push(format!("the silent-session book was not kept: {error}"));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_in_may_wait_longer_than_other_calls() {
        assert_eq!(call_limit(Some((EntryKind::Tool, "mcp__north__finish {}"))), HAND_IN);
        assert_eq!(call_limit(Some((EntryKind::Tool, "Bash cargo test"))), CALL);
        assert_eq!(call_limit(None), CALL);
    }
}
