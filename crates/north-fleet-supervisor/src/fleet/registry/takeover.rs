use std::path::PathBuf;

use crate::error::NorthResult;

use super::Fleet;
use super::stopping::Halt;
use super::super::model::{Role, SessionRecord};
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
        if claim != Claim::TakeOver || !self.session_dir(name)?.exists() {
            return Ok(None);
        }
        let record = self.record(name)?;
        let archived = self.archive(name, Halt::TakenOver, Halt::TakenOver.label())?;
        eprintln!("north fleet: {name} was held by conversation {}; it is stopped and archived at {}, and the name and its messages now go to the new session", record.conversation.as_deref().unwrap_or("none"), archived.display());
        Ok(Some(Predecessor { record }))
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
    use std::sync::mpsc;
    use std::time::Duration;

    use super::super::HandedIn;
    use super::super::fixtures::{fleet, live};
    use super::*;
    use crate::fleet::records::{read_record, write_record};
    use crate::fleet::sender::Sender;
    use crate::fleet::store::RowKind;

    // KEEP(bug): at 4:35 PM on 2026-11-09 a tick take-over archived broker-94 while a row delivery was reviving it, the revive wrote back the old launch the take-over had just retired, and the session showed as exited (north#1017)
    #[test]
    fn a_take_over_racing_a_revive_leaves_one_running_session_in_its_own_conversation() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let dir = fleet.root.join("worker");
        std::fs::create_dir_all(&dir).unwrap();
        let parked = serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "worker", "host_dir": dir, "stream_offset": 0});
        std::fs::write(dir.join("session.json"), parked.to_string()).unwrap();
        let row = fleet.new_row(RowKind::Message, "worker", "carry on with the build", &Sender::Tom).unwrap();
        let (answering, answered) = mpsc::channel();

        let (predecessor, successor, revive) = fleet
            .succeed("worker", Claim::TakeOver, |predecessor| {
                let delivery = fleet.clone();
                let revive = std::thread::spawn(move || {
                    let handed = delivery.hand_in(&row);
                    let _ = answering.send(());
                    handed
                });
                let _ = answered.recv_timeout(Duration::from_millis(300));
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
        let (locking, locked) = mpsc::channel();
        std::thread::spawn(move || {
            let _lock = fleet.start_lock("worker");
            let _ = locking.send(());
        });
        assert!(locked.recv_timeout(Duration::from_millis(300)).is_err(), "the next take-over on this thread left the name unlocked");
    }
}
