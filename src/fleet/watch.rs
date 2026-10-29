use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;

use crate::fleet::session_host::{FrameWatch, Rest, SentLog};

use super::books::WatchBooks;
use super::claims::{self, Claims};
use super::gates::Question;
use super::handoff;
use super::idle::{self, Step};
use super::landings::{self, push_log};
use super::model::{Delivery, EntryKind, SessionRecord, Transcript};
use super::prompt::is_deepened;
use super::receipts::{Courier, Report, deliver_report, is_final};
use super::records::{find_session, host_sessions, parents_live, write_record};
use super::runtime::{Runtime, now_ms};
use super::turns::{RunRecord, Turn, TurnExit, TurnSignal, Turns};
use super::unaccepted::Unaccepted;
use super::wait;

const KEPT_LINES: usize = 5;

pub struct TurnWatch {
    runtime: Arc<dyn Runtime>,
    ledger: Arc<dyn WatchBooks>,
    courier: Arc<dyn Courier>,
    claims: Arc<dyn Claims>,
    root: PathBuf,
    host_dir: PathBuf,
    turns: Turns,
    views: HashMap<String, View>,
    idle: idle::Policy,
    finished: HashMap<String, bool>,
    waits: HashMap<String, Wait>,
    rolled_over: bool,
    nudges: HashMap<String, Option<Nudge>>,
    unaccepted: Arc<SentPrompts>,
}

struct SentPrompts {
    runtime: Arc<dyn Runtime>,
    journal: Mutex<Unaccepted>,
}

impl SentPrompts {
    fn journal(&self) -> MutexGuard<'_, Unaccepted> {
        self.journal.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SentLog for SentPrompts {
    fn sent(&self, line: &str) {
        let Ok(frame) = serde_json::from_str::<Value>(line) else { return };
        if let Some(id) = self.runtime.sent_prompt(&frame) {
            self.journal().sent(id, line.trim_end().to_owned());
        }
    }
}

struct Wait {
    wakes: usize,
    reply: String,
}

struct Nudge {
    threshold: u64,
    last: Option<u64>,
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
    pub fn new(runtime: Arc<dyn Runtime>, ledger: Arc<dyn WatchBooks>, courier: Arc<dyn Courier>, claims: Arc<dyn Claims>, root: PathBuf, host_dir: PathBuf) -> Self {
        let unaccepted = Arc::new(SentPrompts { runtime: runtime.clone(), journal: Mutex::new(Unaccepted::inherit(&host_dir)) });
        Self {
            runtime,
            ledger,
            courier,
            claims,
            root,
            host_dir,
            turns: Turns::default(),
            views: HashMap::new(),
            idle: idle::Policy::default(),
            finished: HashMap::new(),
            waits: HashMap::new(),
            rolled_over: false,
            nudges: HashMap::new(),
            unaccepted,
        }
    }

    #[cfg(test)]
    pub fn idling_by(mut self, policy: idle::Policy) -> Self {
        self.idle = policy;
        self
    }

    fn follow(&mut self, signal: &TurnSignal) {
        match signal {
            TurnSignal::Prompt { conversation, text } => {
                self.finished.insert(conversation.clone(), false);
                if !wait::is_wake(text) {
                    self.waits.remove(conversation);
                }
            }
            TurnSignal::Call { conversation, tokens, .. } => {
                self.rolled_over = false;
                let context = tokens.context();
                let (root, host_dir) = (&self.root, &self.host_dir);
                let armed = self.nudges.entry(conversation.clone()).or_insert_with(|| {
                    find_session(root, host_dir, conversation).and_then(|record| record.handoff_at).map(|threshold| Nudge { threshold, last: None })
                });
                let Some(nudge) = armed.as_mut().filter(|nudge| handoff::due(nudge.threshold, context, nudge.last)) else { return };
                nudge.last = Some(context);
                let threshold = nudge.threshold;
                self.nudge(conversation, context, threshold);
            }
            TurnSignal::Ended { conversation, exit, .. } => {
                let Some(reply) = self.views.get(conversation).and_then(|view| view.reply.clone()).filter(|reply| !reply.trim().is_empty()) else { return };
                self.finished.insert(conversation.clone(), is_final(&reply));
                if *exit == TurnExit::Completed {
                    self.note_wait(conversation, reply);
                }
            }
            _ => {}
        }
    }

    fn note_wait(&mut self, conversation: &str, reply: String) {
        let box_worker = find_session(&self.root, &self.host_dir, conversation).is_some_and(|record| record.task_ref.is_some() && !record.stopped);
        if box_worker && wait::ended_on_a_wait(&reply) {
            self.waits.entry(conversation.to_owned()).or_insert(Wait { wakes: 0, reply: String::new() }).reply = reply;
        } else {
            self.waits.remove(conversation);
        }
    }

    fn wake(&mut self) -> Option<Vec<String>> {
        let wait = self.waits.values_mut().next()?;
        let frames = self.runtime.message(&Transcript::new(), &wait::wake(&wait.reply, wait.wakes)).ok()?;
        wait.wakes += 1;
        Some(frames)
    }

    fn stall(&mut self) {
        let Some((conversation, wait)) = self.waits.drain().next() else { return };
        let Some(record) = find_session(&self.root, &self.host_dir, &conversation) else { return };
        if let Some(parent) = record.parent.as_deref() {
            self.courier.post(parent, wait::stalled(&record.name, &wait.reply), Delivery::Queued);
        }
    }

    fn nudge(&self, conversation: &str, context: u64, threshold: u64) {
        let name = find_session(&self.root, &self.host_dir, conversation).map_or_else(|| conversation.to_owned(), |record| record.name);
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let note = home.join(".local/state/agents/handoffs").join(format!("{name}.md"));
        let Ok(frames) = self.runtime.message(&Transcript::new(), &handoff::nudge(context, threshold, &note)) else { return };
        for frame in frames {
            self.courier.answer(&self.host_dir, frame);
        }
    }

    fn retire(&mut self) -> bool {
        let sessions = host_sessions(&self.root, &self.host_dir);
        let retirable = !sessions.is_empty() && sessions.iter().all(|record| (record.parent.is_some() || record.task_ref.is_some()) && !parents_live(&self.root, &record.name));
        if !retirable {
            self.finished.clear();
            return false;
        }
        for mut record in sessions {
            record.stopped = true;
            if let Err(error) = write_record(&self.root.join(&record.name), &record) {
                eprintln!("north session-host: {} not marked exited: {}", record.name, error.user_message());
            }
        }
        true
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
        if let Some(id) = self.runtime.accepted_prompt(&frame) {
            self.unaccepted.journal().accepted(&id);
        }
        let signals = self.runtime.signals(&frame);
        for signal in &signals {
            self.views.entry(signal.conversation().to_owned()).or_insert_with(|| View::of(signal.conversation()));
        }
        for view in self.views.values_mut() {
            view.observe(self.runtime.as_ref(), &frame);
        }
        for signal in signals {
            self.follow(&signal);
            if let Some(turn) = self.turns.observe(signal, now_ms()) {
                self.record(turn);
            }
        }
    }

    fn sent_log(&self) -> Arc<dyn SentLog> {
        self.unaccepted.clone()
    }

    fn in_flight(&self) -> bool {
        self.turns.in_flight()
    }

    fn rest(&mut self, idle: Duration, park_after: Option<Duration>) -> Rest {
        let finished = !self.finished.is_empty() && self.finished.values().all(|finished| *finished);
        let wakes = self.waits.values().map(|wait| wait.wakes).next();
        let session = idle::Session { idle, in_flight: self.turns.in_flight(), context: self.turns.context(), rolled_over: self.rolled_over, finished, wakes };
        match self.idle.step(session, park_after) {
            Step::Wait => Rest::Wait,
            Step::Wake => self.wake().map_or(Rest::Wait, Rest::Send),
            Step::Stall => {
                self.stall();
                self.rest(idle, park_after)
            }
            Step::Park => Rest::End,
            Step::Compact => {
                self.rolled_over = true;
                self.runtime.compact().map_or(Rest::Wait, |frame| Rest::Send(vec![frame]))
            }
            Step::Exit if self.retire() => Rest::End,
            Step::Exit => self.rest(idle, park_after),
        }
    }

    fn close(&mut self) {
        for turn in self.turns.abandon(now_ms()) {
            self.record(turn);
        }
        if !self.finished.is_empty() && self.finished.values().all(|finished| *finished) && self.unaccepted.journal().is_empty() {
            self.retire();
        }
        for record in host_sessions(&self.root, &self.host_dir) {
            claims::release_for(self.claims.as_ref(), &record);
        }
    }
}
