use std::fmt;
use std::time::Duration;

use crate::error::NorthResult;
use crate::fleet::model::{Delivery, LastSender, SessionRecord};
use crate::fleet::records::{read_record, write_record};
use crate::fleet::runtime::{SESSION_ENV, now_ms};
use crate::fleet::transcript::stream_len;

use super::{Fleet, await_reply};

const RIVAL_WINDOW_MS: u64 = 30 * 60 * 1000;

#[derive(Debug, Eq, PartialEq)]
pub struct Rival {
    pub to: String,
    pub name: String,
    pub minutes_ago: u64,
}

impl fmt::Display for Rival {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "north fleet: {} also messaged {} {}m ago; make sure your orders agree with theirs", self.name, self.to, self.minutes_ago)
    }
}

impl Fleet {
    pub fn caller(&self) -> Option<String> {
        caller_name(std::env::var(SESSION_ENV).ok(), self.runtimes.caller_conversation(), || self.sessions().unwrap_or_default())
    }

    pub fn rival(&self, from: &str, to: &str) -> Option<Rival> {
        let last = self.record(to).ok()?.last_sender?;
        rival(&last, from, to, now_ms())
    }

    pub fn send_signed(&self, from: Option<&str>, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        self.send(name, &signed(from, text), delivery)?;
        if let Some(from) = from
            && let Err(error) = self.note_sender(name, from)
        {
            eprintln!("north fleet: delivered to {name}, but its sender was not recorded: {}", error.user_message());
        }
        Ok(())
    }

    pub fn ask(&self, from: Option<&str>, name: &str, question: &str, timeout: Duration) -> NorthResult<Option<String>> {
        let record = self.revive(name, None)?;
        let asked_from = stream_len(&record.host_dir);
        self.send_signed(from, name, question, Delivery::Now)?;
        await_reply(self.runtime(&record)?.as_ref(), &record, asked_from, question, timeout)
    }

    fn note_sender(&self, name: &str, from: &str) -> NorthResult<()> {
        let session_dir = self.session_dir(name)?;
        let mut record = read_record(&session_dir)?;
        record.last_sender = Some(LastSender { name: from.to_owned(), at_ms: now_ms() });
        write_record(&session_dir, &record)
    }
}

fn signed(from: Option<&str>, text: &str) -> String {
    match from {
        Some(from) => format!("[from {from}] {text}"),
        None => text.to_owned(),
    }
}

fn caller_name(named: Option<String>, conversation: Option<String>, sessions: impl FnOnce() -> Vec<SessionRecord>) -> Option<String> {
    if let Some(named) = named.filter(|name| !name.is_empty()) {
        return Some(named);
    }
    let conversation = conversation?;
    let registered = sessions().into_iter().find(|record| record.conversation.as_deref() == Some(conversation.as_str())).map(|record| record.name);
    Some(registered.unwrap_or(conversation))
}

fn rival(last: &LastSender, from: &str, to: &str, now: u64) -> Option<Rival> {
    let age = now.saturating_sub(last.at_ms);
    (last.name != from && age < RIVAL_WINDOW_MS).then(|| Rival { to: to.to_owned(), name: last.name.clone(), minutes_ago: age / 60_000 })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::fleet::runtime::Runtimes;

    fn fleet(home: &std::path::Path) -> Fleet {
        let ledger = Arc::new(crate::ledger::Ledger::open(&home.join("ledger.db")).unwrap());
        Fleet::at(home.into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap()
    }

    fn lead(fleet: &Fleet, name: &str, conversation: &str) -> SessionRecord {
        let record: SessionRecord = serde_json::from_value(serde_json::json!({
            "name": name, "provider": "claude", "account": "a", "cwd": "/tmp",
            "model": null, "created_ms": 1, "host_pid": null, "conversation": conversation,
        }))
        .unwrap();
        let session_dir = fleet.session_dir(name).unwrap();
        std::fs::create_dir_all(&session_dir).unwrap();
        write_record(&session_dir, &record).unwrap();
        record
    }

    #[test]
    fn a_caller_is_its_fleet_name_else_the_registered_name_of_its_conversation_else_the_conversation() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let sessions = vec![lead(&fleet, "proxy", "d11ef811")];
        assert_eq!(caller_name(Some("worker".into()), Some("d11ef811".into()), || panic!("a named caller reads no registry")).as_deref(), Some("worker"));
        assert_eq!(caller_name(Some(String::new()), Some("d11ef811".into()), || sessions.clone()).as_deref(), Some("proxy"));
        assert_eq!(caller_name(None, Some("28c1c1d1".into()), || sessions.clone()).as_deref(), Some("28c1c1d1"));
        assert_eq!(caller_name(None, None, || sessions.clone()), None);
        assert_eq!(signed(Some("proxy"), "staff #70"), "[from proxy] staff #70");
        assert_eq!(signed(None, "staff #70"), "staff #70");
    }

    #[test]
    fn a_send_to_a_lead_another_sender_messaged_within_thirty_minutes_names_that_sender() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        lead(&fleet, "wisp-lead", "c0ffee");
        assert_eq!(fleet.rival("proxy", "wisp-lead"), None, "a lead nobody messaged has no rival");

        fleet.note_sender("wisp-lead", "28c1c1d1").unwrap();
        assert_eq!(fleet.session("wisp-lead").unwrap().last_sender.map(|last| last.name).as_deref(), Some("28c1c1d1"));
        let warned = fleet.rival("proxy", "wisp-lead").expect("a fresh order from another sender warns");
        assert_eq!((warned.name.as_str(), warned.minutes_ago), ("28c1c1d1", 0));
        assert!(warned.to_string().contains("28c1c1d1 also messaged wisp-lead 0m ago"), "{warned}");
        assert_eq!(fleet.rival("28c1c1d1", "wisp-lead"), None, "the same sender is not its own rival");

        let last = LastSender { name: "28c1c1d1".into(), at_ms: 1_000 };
        assert_eq!(rival(&last, "proxy", "wisp-lead", 1_000 + RIVAL_WINDOW_MS - 60_000).map(|rival| rival.minutes_ago), Some(29));
        assert_eq!(rival(&last, "proxy", "wisp-lead", 1_000 + RIVAL_WINDOW_MS), None, "an order older than 30 minutes is not a rival");
    }

    #[test]
    fn senders_recording_on_one_lead_at_once_all_succeed() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        lead(&fleet, "wisp-lead", "c0ffee");
        std::thread::scope(|scope| {
            let writers: Vec<_> = (0..8).map(|n| scope.spawn({
                let fleet = &fleet;
                move || fleet.note_sender("wisp-lead", &format!("proxy-{n}"))
            })).collect();
            for writer in writers {
                writer.join().unwrap().unwrap();
            }
        });
        assert!(fleet.session("wisp-lead").unwrap().last_sender.is_some_and(|last| last.name.starts_with("proxy-")));
    }
}
