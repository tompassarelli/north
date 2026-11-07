use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::error::NorthResult;
use crate::fleet::forget;
use crate::fleet::jobs::Lease;
use crate::fleet::model::{Role, SessionRecord, Status};
use crate::fleet::records::{ARCHIVE, RETIRED_NOTE as NOTE, read_record};
use crate::fleet::runtime::now_ms;
use crate::fleet::store::{FleetRow, LeaseTable, cache_leases, cached_leases};

use super::Fleet;
use super::stopping::Halt;

enum Asking {
    Known(Lease),
    Asked(mpsc::Receiver<NorthResult<bool>>),
}

const DEAD_LETTERS: &str = ".dead-letters.jsonl";
const MOST_HOPS: usize = 16;

impl Fleet {
    pub fn retire(&self, name: &str, why: &str, last: Option<&str>) -> NorthResult<PathBuf> {
        let note = json!({"why": why, "at_ms": now_ms(), "last": last});
        fs::write(self.session_dir(name)?.join(NOTE), serde_json::to_vec_pretty(&note)?)?;
        let stop = last.and_then(|last| last.lines().rev().map(str::trim).find(|line| line.starts_with("Stop:")));
        let cause = stop.map_or_else(|| why.to_owned(), |stop| format!("{why}; {stop}"));
        let archived = self.archive(name, Halt::Retired, &cause)?;
        self.uncache_session(name)?;
        Ok(archived)
    }

    pub fn retired_record(&self, name: &str) -> Option<SessionRecord> {
        self.retired_session(name).map(|(_, record)| record)
    }

    /// The names archived at or after `since_ms`, from the stamp each archive directory carries, and the newest archived
    /// session of each task whatever its age, since dispatch reads the last launch of each open issue however old.
    pub fn retired_names_read(&self, since_ms: u64) -> Vec<String> {
        let mut newest: std::collections::BTreeMap<String, (u64, String)> = std::collections::BTreeMap::new();
        let mut names = std::collections::BTreeSet::new();
        for entry in fs::read_dir(self.root.join(ARCHIVE)).into_iter().flatten().flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some((name, stamp)) = file.rsplit_once('-').and_then(|(name, stamp)| Some((name, stamp.parse::<u64>().ok()?))) else { continue };
            if stamp >= since_ms {
                names.insert(name.to_owned());
            }
            let Some(record) = fs::read_to_string(entry.path().join("session.json")).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()) else { continue };
            let created = record["created_ms"].as_u64().unwrap_or(0);
            let tasks = record["task_ref"].as_str().into_iter().chain(record["extra_tasks"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_str));
            for task in tasks {
                if newest.get(task).is_none_or(|(seen, _)| *seen < created) {
                    newest.insert(task.to_owned(), (created, name.to_owned()));
                }
            }
        }
        names.extend(newest.into_values().map(|(_, name)| name));
        names.into_iter().collect()
    }

    pub fn retired_session(&self, name: &str) -> Option<(PathBuf, SessionRecord)> {
        let prefix = format!("{name}-");
        fs::read_dir(self.root.join(ARCHIVE))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().strip_prefix(&prefix).is_some_and(|stamp| stamp.chars().all(|c| c.is_ascii_digit())))
            .filter_map(|entry| Some((entry.path(), read_record(&entry.path()).ok()?)))
            .filter(|(_, record)| record.name == name)
            .max_by_key(|(_, record)| record.created_ms)
    }

    pub(super) fn heir(&self, name: &str) -> NorthResult<Option<String>> {
        if self.session_dir(name)?.exists() || self.known_elsewhere(name)? {
            return Ok(None);
        }
        let Some(mut record) = self.retired_record(name) else { return Ok(None) };
        for _ in 0..MOST_HOPS {
            let Some(parent) = record.parent.clone() else { break };
            if self.session_dir(&parent)?.exists() || self.known_elsewhere(&parent)? {
                return Ok(Some(parent));
            }
            match self.retired_record(&parent) {
                Some(retired) => record = retired,
                None => break,
            }
        }
        Ok(self.sessions()?.into_iter().find(|session| session.role() == Role::Proxy && !session.ended()).map(|proxy| proxy.name))
    }

    pub(super) fn dead_letter(&self, name: &str, heir: &str, text: &str) -> String {
        let line = json!({"at_ms": now_ms(), "to": name, "heir": heir});
        if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(self.root.join(DEAD_LETTERS)) {
            let _ = writeln!(log, "{line}");
        }
        format!("[north: {name} is retired, so this message to it comes to you] {text}")
    }

    pub(super) fn hand_in_to_heir(&self, row: &FleetRow, heir: &str) -> NorthResult<super::HandedIn> {
        let rerouted = FleetRow { to: heir.to_owned(), text: self.dead_letter(&row.to, heir, &row.text), ..row.clone() };
        if self.session_dir(heir)?.exists() {
            return self.hand_in(&rerouted);
        }
        self.spool().put(&rerouted)?;
        Ok(super::HandedIn::Delivered)
    }

    pub(crate) fn live_wait(&self, record: &SessionRecord, all: &[SessionRecord], gated: &BTreeSet<String>, running: bool) -> Option<String> {
        let name = record.name.as_str();
        if let Some(wait) = self.waits().of(name).first() {
            return Some(format!("waits on {}", wait.run));
        }
        self.live_hold(record, all, gated).or_else(|| running.then(|| "a background job still runs".into()))
    }

    pub(crate) fn live_hold(&self, record: &SessionRecord, all: &[SessionRecord], gated: &BTreeSet<String>) -> Option<String> {
        let name = record.name.as_str();
        if gated.contains(name) {
            return Some("has an open gate".into());
        }
        if let Some(lane) = forget::queued_lanes(&self.state.join("land-queue"), name, Some(&record.cwd)).first() {
            return Some(format!("has lane {} queued", lane.branch));
        }
        if let Some(child) = all.iter().find(|child| child.parent.as_deref() == Some(name) && self.status(child) != Status::Exited) {
            return Some(format!("has live child {}", child.name));
        }
        None
    }

    pub(crate) fn job_leases(&self, records: &[&SessionRecord], now_ms: u64, within: Duration) -> Vec<Lease> {
        let cached = cached_leases(&self.root).ok().flatten();
        let deadline = Instant::now().checked_add(within);
        let asking: Vec<Asking> = records.iter().map(|record| match cached.as_ref().and_then(|table| table.leased(&record.name, now_ms)) {
            Some(leased) => Asking::Known(Lease::read(Ok(leased))),
            None => self.ask_lease(record),
        }).collect();
        asking.into_iter().map(|asked| match asked {
            Asking::Known(lease) => lease,
            Asking::Asked(answered) => match deadline {
                Some(deadline) => answered.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_or(Lease::Unknown, Lease::read),
                None => answered.recv().map_or(Lease::Unknown, Lease::read),
            },
        }).collect()
    }

    fn ask_lease(&self, record: &SessionRecord) -> Asking {
        let (answer, answered) = mpsc::channel();
        let (leases, name, machine) = (self.leases.clone(), record.name.clone(), record.machine.clone());
        let asked = std::thread::Builder::new().name("reap.lease".into()).spawn(move || answer.send(leases.leased(&name, machine.as_ref())));
        asked.map_or(Asking::Known(Lease::Unknown), |_| Asking::Asked(answered))
    }

    pub fn cache_leases(&self, table: &LeaseTable) -> NorthResult<()> {
        cache_leases(&self.root, table)
    }

    pub(crate) fn open_gate_sessions(&self) -> BTreeSet<String> {
        self.ledger.gates(u64::MAX).map(|gates| gates.into_iter().filter(|gate| gate.is_open()).map(|gate| gate.request.session).collect()).unwrap_or_default()
    }

    pub(crate) fn report_taken(&self, record: &SessionRecord) -> bool {
        let Some(parent) = record.parent.as_deref() else { return true };
        let gone = self.session_dir(parent).is_ok_and(|dir| !dir.exists()) && !self.known_elsewhere(parent).unwrap_or(true);
        gone || self.ledger.reported(&record.name).unwrap_or(false)
    }
}
