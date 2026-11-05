use std::fmt;
use std::time::Duration;

use crate::error::NorthResult;
use crate::fleet::model::{Delivery, LastSender, SessionRecord};
use crate::fleet::records::{read_record, write_record};
use crate::fleet::runtime::{SESSION_ENV, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::Spool;
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

    pub fn rival(&self, from: &Sender, to: &str) -> Option<Rival> {
        let last = self.record(to).ok()?.last_sender?;
        rival(&last, from.session()?, to, now_ms())
    }

    pub fn send_signed(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        self.record(name)?;
        let row = self.spool_message(from, name, text, delivery)?;
        self.hand_in(&row).map(drop).inspect_err(|_| {
            if let Err(error) = Spool::at(&self.root).withdraw(&row.id) {
                eprintln!("north fleet: row {} was not withdrawn from the spool: {}", row.id, error.user_message());
            }
        })
    }

    pub fn ask(&self, from: &Sender, name: &str, question: &str, timeout: Duration) -> NorthResult<Option<String>> {
        let record = self.revive(name, None)?;
        let asked_from = stream_len(&record.host_dir);
        self.send_signed(from, name, question, Delivery::Now)?;
        await_reply(self.runtime(&record)?.as_ref(), &record, asked_from, question, timeout)
    }

    pub(super) fn note_sender(&self, name: &str, from: &Sender) -> NorthResult<()> {
        let session_dir = self.session_dir(name)?;
        let mut record = read_record(&session_dir)?;
        if let Some(session) = from.session() {
            record.last_sender = Some(LastSender { name: session.to_owned(), at_ms: now_ms() });
        }
        record.prompt_sender = Some(from.clone());
        write_record(&session_dir, &record)
    }
}

pub(super) fn signed(from: &Sender, text: &str) -> String {
    match from.prefix() {
        Some(prefix) => format!("[from {prefix}] {text}"),
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

