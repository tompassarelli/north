use std::fmt;
use std::time::Duration;

use crate::error::NorthResult;
use crate::fleet::model::{Delivery, LastSender, Role, SessionRecord};
use crate::fleet::records::{read_record, write_record};
use crate::fleet::runtime::{SESSION_ENV, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::Spool;
use crate::fleet::transcript::stream_len;

use super::{Fleet, Sent, await_reply};

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

    pub fn answerer(&self) -> Option<String> {
        session_name(std::env::var(SESSION_ENV).ok(), self.runtimes.caller_conversation().as_deref(), || self.sessions().unwrap_or_default())
    }

    pub fn rival(&self, from: &Sender, to: &str) -> Option<Rival> {
        let last = self.record(to).ok()?.last_sender?;
        rival(&last, from.session()?, to, now_ms())
    }

    pub fn send_signed(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        self.hand_in_signed(from, name, text, delivery).map(drop)
    }

    /// Spools the message and hands it in, withdrawing the row when the hand-in fails. A process that may not journal the hand-in, such as a worker sandbox's command, hands it over unspooled instead.
    pub(super) fn hand_in_signed(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        let record = self.record(name)?;
        if !self.journals_deliveries() {
            return self.hand_over(from, &record, text, delivery);
        }
        let row = self.spool_message(from, name, text, delivery)?;
        self.hand_in(&row).map(|_| Sent::HandedIn).inspect_err(|_| {
            if let Err(error) = Spool::at(&self.root).withdraw(&row.id) {
                eprintln!("north fleet: row {} was not withdrawn from the spool: {}", row.id, error.user_message());
            }
        })
    }

    /// Spools the row, then prompts the live host of `record` on this host with the row's own prompt id and leaves the row in the spool, so the fleet tick, finding the host already holds the prompt, records the sender and the delivery instead of prompting again. A sandbox cannot write the delivery lock, so this direct prompt is best effort: it can race a tick's delivery of the same row. A proxy, an adopted window behind the terminal multiplexer's hidden sockets and a session with no live host are left to the tick alone.
    fn hand_over(&self, from: &Sender, record: &SessionRecord, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        let row = self.spool_message(from, &record.name, text, delivery)?;
        if record.live() && !record.adopted() && record.role() != Role::Proxy {
            match self.prompt_live(record, &row.prompt_id(), &from.sign(text), delivery) {
                Ok(()) => return Ok(Sent::HandedIn),
                Err(error) => eprintln!("north fleet: {} did not take the message, so the fleet tick hands it in: {}", record.name, error.user_message()),
            }
        }
        Ok(Sent::Queued { id: row.id })
    }

    /// Spools the message and hands it in; a hand-in that fails, or that this process may not journal, leaves the row in the spool, which the next tick delivers.
    pub fn send_spooled(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        self.record(name)?;
        let row = self.spool_message(from, name, text, delivery)?;
        if !self.journals_deliveries() {
            return Ok(());
        }
        if let Err(error) = self.hand_in(&row) {
            eprintln!("north fleet: row {} for {name} stays in the spool for the next tick: {}", row.id, error.user_message());
        }
        Ok(())
    }

    pub fn ask(&self, from: &Sender, name: &str, question: &str, timeout: Duration) -> NorthResult<Option<String>> {
        let record = self.wake(name)?;
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

fn caller_name(named: Option<String>, conversation: Option<String>, sessions: impl FnOnce() -> Vec<SessionRecord>) -> Option<String> {
    session_name(named, conversation.as_deref(), sessions).or(conversation)
}

fn session_name(named: Option<String>, conversation: Option<&str>, sessions: impl FnOnce() -> Vec<SessionRecord>) -> Option<String> {
    if let Some(named) = named.filter(|name| !name.is_empty()) {
        return Some(named);
    }
    let conversation = conversation?;
    sessions().into_iter().find(|record| record.conversation.as_deref() == Some(conversation)).map(|record| record.name)
}

fn rival(last: &LastSender, from: &str, to: &str, now: u64) -> Option<Rival> {
    let age = now.saturating_sub(last.at_ms);
    (last.name != from && age < RIVAL_WINDOW_MS).then(|| Rival { to: to.to_owned(), name: last.name.clone(), minutes_ago: age / 60_000 })
}

