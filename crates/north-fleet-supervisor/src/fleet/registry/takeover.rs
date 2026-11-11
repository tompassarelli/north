use std::path::PathBuf;

use crate::error::NorthResult;

use super::Fleet;
use super::stopping::Halt;
use super::super::model::{Role, SessionRecord};
use super::super::records::record_file;
use super::super::runtime::PROXY;
use super::super::store::{Addressee, Claim, FleetRow, RowKind};

/// The session a takeover ended, and where its directory now lies.
pub(super) struct Predecessor {
    record: SessionRecord,
}

impl Predecessor {
    /// The successor takes every message to the name from where the predecessor's line began, so a report sent before the takeover that the old session never took still arrives.
    pub(super) fn record(&self) -> &SessionRecord {
        &self.record
    }

    pub(super) fn addressed_since(&self) -> u64 {
        self.record.addressed_since()
    }
}

impl Fleet {
    /// Ends and archives the session holding `name` when this launch takes the name over; None when no session holds it.
    pub(super) fn retire_for(&self, name: &str, claim: Claim) -> NorthResult<Option<Predecessor>> {
        if claim != Claim::TakeOver || !self.session_dir(name)?.exists() || self.clear_ghost(name)? {
            return Ok(None);
        }
        let record = self.record(name)?;
        let archived = self.archive(name, Halt::TakenOver, Halt::TakenOver.label())?;
        eprintln!("north fleet: {name} was held by conversation {}; it is stopped and archived at {}, and the name and its messages now go to the new session", record.conversation.as_deref().unwrap_or("none"), archived.display());
        Ok(Some(Predecessor { record }))
    }

    /// Removes a directory that holds a name with no session record in it, as a launch killed after claiming the name leaves, and gives up the claims the dead launch took under the name; true when it removed one. The caller holds the name's start lock, so no live launch is building a record there.
    pub(super) fn clear_ghost(&self, name: &str) -> NorthResult<bool> {
        let dir = self.session_dir(name)?;
        if !dir.exists() || record_file(&dir).exists() {
            return Ok(false);
        }
        if let Err(error) = self.release_all_claims_of(name) {
            eprintln!("north fleet: a launch of {name} died before its session began, and its claims were not all given up: {}", error.user_message());
        }
        self.discard_failed_launch(&dir)?;
        eprintln!("north fleet: {name} was left by a launch that died before its session began; its directory is removed and the name is free");
        Ok(true)
    }

    /// Retires the session holding `name` for a take-over and runs `succeed` to bring up its successor, all under the name's start lock, so a revive racing them either runs before the old session is stopped or finds the successor running.
    pub(super) fn succeed<T>(&self, name: &str, claim: Claim, succeed: impl FnOnce(Option<Predecessor>) -> NorthResult<T>) -> NorthResult<T> {
        let _start = self.start_lock(name)?;
        succeed(self.retire_for(name, claim)?)
    }

    pub fn fleet_proxy(&self) -> NorthResult<Option<SessionRecord>> {
        Ok(self.sessions()?.into_iter().find(|record| record.name == PROXY && record.role == Some(Role::Proxy)))
    }

    pub fn earlier_proxies(&self) -> NorthResult<Vec<SessionRecord>> {
        let earlier = |record: &SessionRecord| match record.role {
            None => record.name == PROXY || record.name.starts_with("proxy-"),
            Some(role) => role == Role::Proxy && record.name != PROXY,
        };
        Ok(self.sessions()?.into_iter().filter(earlier).collect())
    }

    pub fn retire_proxy(&self, name: &str) -> NorthResult<PathBuf> {
        self.archive(name, Halt::Retired, "a proxy take-over retired it")
    }

    pub fn addressee(&self, record: &SessionRecord, floor: u64) -> NorthResult<Addressee> {
        let taken = self.delivered(&record.name)?.into_iter().filter(|id| sent_at(id).is_none_or(|ms| ms >= floor)).collect();
        Ok(Addressee { name: record.name.clone(), stopped: record.ended(), control_since_ms: Some(record.created_ms.max(floor)), since_ms: record.addressed_since().max(floor), taken })
    }
}

pub fn backlog_line(retired: &[String], rows: &[FleetRow], now_ms: u64) -> String {
    let messages: Vec<&FleetRow> = rows.iter().filter(|row| row.kind == RowKind::Message).collect();
    let names = retired.join(" and ");
    let Some(oldest) = messages.iter().map(|row| row.sent_at).min() else {
        return format!("[north: north proxy archived {names}; no messages to them were waiting]");
    };
    let mut senders: Vec<String> = messages.iter().map(|row| row.sender.prefix().unwrap_or_else(|| "Tom".to_owned())).collect();
    senders.sort();
    senders.dedup();
    let count = messages.len();
    format!(
        "[north: north proxy archived {names}; {count} message{} to them were not replayed, from {}, the oldest {} old]",
        if count == 1 { "" } else { "s" },
        senders.join(", "),
        age(now_ms.saturating_sub(oldest))
    )
}

fn age(ms: u64) -> String {
    let minutes = ms / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

fn sent_at(id: &str) -> Option<u64> {
    id.split('-').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::super::HandedIn;
    use super::super::fixtures::{fleet, live};
    use super::*;
    use crate::fleet::records::{read_record, write_record};
    use crate::fleet::sender::Sender;
    use crate::fleet::store::RowKind;

    #[test]
    fn a_take_over_racing_a_revive_leaves_one_running_session_in_its_own_conversation() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let dir = fleet.root.join("worker");
        std::fs::create_dir_all(&dir).unwrap();
        let parked = serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "worker", "host_dir": dir, "stream_offset": 0});
        std::fs::write(dir.join("session.json"), parked.to_string()).unwrap();
        let row = fleet.new_row(RowKind::Message, "worker", "carry on with the build", &Sender::Tom).unwrap();

        let (predecessor, successor, revive) = fleet
            .succeed("worker", Claim::TakeOver, |predecessor| {
                let delivery = fleet.clone();
                let revive = std::thread::spawn(move || delivery.hand_in(&row));
                let session = live(&fleet, "worker");
                let mut successor = read_record(&dir).unwrap();
                successor.created_ms = 2;
                write_record(&dir, &successor).unwrap();
                Ok((predecessor.map(|predecessor| predecessor.record().created_ms), session, revive))
            })
            .unwrap();

        assert_eq!(revive.join().unwrap().unwrap(), HandedIn::Delivered);
        assert_eq!(predecessor, Some(1));
        let running: Vec<SessionRecord> = fleet.sessions().unwrap().into_iter().filter(|record| record.name == "worker").collect();
        assert!(matches!(running.as_slice(), [record] if record.live() && record.created_ms == 2 && record.conversation.as_deref() == Some("worker")), "{running:?}");
        assert!(successor.frames.lock().unwrap().iter().any(|frame| frame.contains("carry on with the build")), "the message never reached the successor");
        let _next = fleet.start_lock("worker").unwrap();
        let free = std::thread::spawn(move || fleet.try_start_lock("worker").unwrap().is_some()).join().unwrap();
        assert!(!free, "the next take-over on this thread left the name unlocked");
    }
}
