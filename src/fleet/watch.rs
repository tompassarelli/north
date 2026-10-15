use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::session_host::FrameWatch;

use super::books::WatchBooks;
use super::gates::Question;
use super::landings::{self, push_log};
use super::model::{EntryKind, SessionRecord, Transcript};
use super::prompt::is_deepened;
use super::receipts::{Courier, Report, deliver_report};
use super::records::find_session;
use super::runtime::{Runtime, now_ms};
use super::turns::{RunRecord, Turn, Turns};

const KEPT_LINES: usize = 5;

pub struct TurnWatch {
    runtime: Arc<dyn Runtime>,
    ledger: Arc<dyn WatchBooks>,
    courier: Arc<dyn Courier>,
    root: PathBuf,
    host_dir: PathBuf,
    turns: Turns,
    views: HashMap<String, View>,
}

struct View {
    transcript: Transcript,
    reply: Option<String>,
}

impl View {
    fn of(conversation: &str) -> Self {
        let mut transcript = Transcript::new();
        transcript.conversation = Some(conversation.to_owned());
        Self { transcript, reply: None }
    }

    fn observe(&mut self, runtime: &dyn Runtime, frame: &Value) {
        let seen = self.transcript.entries.len();
        runtime.observe(frame, &mut self.transcript);
        if let Some(reply) = self.transcript.entries[seen..].iter().rev().find(|entry| entry.kind == EntryKind::Assistant) {
            self.reply = Some(reply.text.clone());
        }
        let surplus = self.transcript.entries.len().saturating_sub(KEPT_LINES);
        self.transcript.entries.drain(..surplus);
    }
}

impl TurnWatch {
    pub fn new(runtime: Arc<dyn Runtime>, ledger: Arc<dyn WatchBooks>, courier: Arc<dyn Courier>, root: PathBuf, host_dir: PathBuf) -> Self {
        Self {
            runtime,
            ledger,
            courier,
            root,
            host_dir,
            turns: Turns::default(),
            views: HashMap::new(),
        }
    }

    fn record(&mut self, mut turn: Turn) {
        let session = find_session(&self.root, &self.host_dir, &turn.conversation);
        if turn.model.is_none() {
            turn.model = session.as_ref().and_then(|record| record.model.clone());
        }
        let run = RunRecord {
            session: session.as_ref().map_or_else(|| turn.conversation.clone(), |record| record.name.clone()),
            task_ref: session.as_ref().and_then(|record| record.task_ref.clone()),
            provider: self.runtime.provider().label().to_owned(),
            account: session.as_ref().map(|record| record.account.clone()),
            system_prompt_sha: session.as_ref().and_then(|record| record.system_prompt_sha.clone()),
            deepen: turn.prompt.as_deref().is_some_and(is_deepened),
            turn,
        };
        let number = self.ledger.record_run(&run).map_err(|error| eprintln!("north ledger: {}", error.user_message())).ok();
        let Some(record) = session else { return };
        self.observe_landing(&record);
        if let Some(number) = number {
            self.report(&record, &run.turn, number);
        }
    }

    fn report(&mut self, record: &SessionRecord, turn: &Turn, number: u64) {
        let view = self.views.remove(&turn.conversation);
        let Some(parent) = record.parent.as_deref() else { return };
        let (reply, lines) = view.map(|view| (view.reply, view.transcript.entries)).unwrap_or_default();
        let landed = landings::landed_between(record, &push_log(), turn.started_ms, turn.ended_ms);
        let report = Report { child: record.name.clone(), turn: number, exit: turn.exit, landed, reply };
        if let Err(error) = deliver_report(self.ledger.as_ref(), self.courier.as_ref(), parent, &report, &lines, now_ms()) {
            eprintln!("north fleet: receipt to {parent} not sent: {}", error.user_message());
        }
    }

    fn relay(&self, question: Question) {
        let Some(record) = find_session(&self.root, &self.host_dir, &question.conversation) else { return };
        for ask in &question.asks {
            self.courier.ask(ask.gate(&record.name));
        }
        self.courier.answer(&self.host_dir, question.reply);
    }

    fn observe_landing(&self, record: &SessionRecord) {
        if let Err(error) = landings::observe(self.ledger.as_ref(), record, &push_log()) {
            eprintln!("north ledger: {}", error.user_message());
        }
    }
}

impl FrameWatch for TurnWatch {
    fn line(&mut self, line: &[u8]) {
        let Ok(frame) = serde_json::from_slice::<Value>(line) else { return };
        if let Some(question) = self.runtime.question(&frame) {
            self.relay(question);
        }
        let signals = self.runtime.signals(&frame);
        for signal in &signals {
            self.views.entry(signal.conversation().to_owned()).or_insert_with(|| View::of(signal.conversation()));
        }
        for view in self.views.values_mut() {
            view.observe(self.runtime.as_ref(), &frame);
        }
        for signal in signals {
            if let Some(turn) = self.turns.observe(signal, now_ms()) {
                self.record(turn);
            }
        }
    }

    fn in_flight(&self) -> bool {
        self.turns.in_flight()
    }

    fn close(&mut self) {
        for turn in self.turns.abandon(now_ms()) {
            self.record(turn);
        }
    }
}
