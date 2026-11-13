use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;

use north_base::lifecycle::State as _;
use north_base::platform::attribution::{self, Cause, Harness};

use super::books::WatchBooks;
use super::claims::Claims;
use super::checkpoint::{self, Checkpoints};
use super::detached::{NoScope, Scope};
use super::idle;
use super::jobs::{HeldLeases, JobLeases};
use super::model::{Delivery, Entry, EntryKind, Machine, Phase, Phases, Role, SessionRecord, Transcript};
use super::promises::{Reckon, Unreckoned};
use super::receipts::{Courier, is_final, retires};
use super::records::{find_session, host_sessions, parents_live};
use super::run_recording::{NoRecording, RunRecording};
use super::runtime::{Runtime, new_uuid, now_ms};
use super::sender::Sender;
use super::silent::{self, Quiet, Silence, State};
use super::stranded::{Checkout, Lane, Lanes, Worktree};
use super::tickets::{NoTickets, Tickets};
use super::turns::{TurnSignal, Turns};
use super::unaccepted::Unaccepted;
use super::usage_sink::{NoUsage, UsageSink};
use super::wait;
use super::waits::Waits;

mod frames;
mod handover;
mod records;

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
    rolled_over: bool,
    checkpoints: Checkpoints,
    compacted: HashMap<String, HashSet<String>>,
    unaccepted: Arc<SentPrompts>,
    lanes: Arc<dyn Lanes + Send + Sync>,
    scope: Arc<dyn Scope>,
    quiet: HashMap<String, Quiet>,
    jobs: Arc<dyn JobLeases>,
    tickets: Arc<dyn Tickets>,
    closing: bool,
    usage: Arc<dyn UsageSink>,
    recording: Arc<dyn RunRecording>,
    verdicts: PathBuf,
    reckon: Arc<dyn Reckon>,
}

struct NoLanes;

impl Lanes for NoLanes {
    fn checkout(&self, _machine: Option<&Machine>, _root: &Path) -> Option<Checkout> {
        None
    }

    fn lanes(&self, _machine: Option<&Machine>, _trees: &[Worktree]) -> Vec<Lane> {
        Vec::new()
    }
}

struct Tell {
    lanes: Arc<dyn Lanes + Send + Sync>,
    courier: Arc<dyn Courier>,
    ledger: Arc<dyn WatchBooks>,
    jobs: Arc<dyn JobLeases>,
    record: SessionRecord,
    parent: String,
    state: State,
    turn: u64,
    reply: String,
    cancelled: Arc<AtomicBool>,
    quiet: Duration,
    eta_min: Option<u64>,
    lines: Vec<Entry>,
}

impl Tell {
    fn lane(&self) -> Option<Lane> {
        let machine = self.record.machine.as_ref();
        let tree = self.lanes.checkout(machine, &self.record.cwd)?.worktrees.into_iter().find(|tree| tree.path == self.record.cwd).unwrap_or(Worktree { path: self.record.cwd.clone(), branch: None });
        self.lanes.lanes(machine, std::slice::from_ref(&tree)).into_iter().next()
    }

    /// Posts the note and reports whether the turn is settled; a lane read that failed at idle is tried again at park or exit.
    fn run(self) -> bool {
        let leased = self.jobs.leased(&self.record.name, self.record.machine.as_ref()).unwrap_or_else(|error| {
            eprintln!("north fleet: the job leases of {} were not read: {}", self.record.name, error.user_message());
            true
        });
        if leased {
            return false;
        }
        let wants_lane = silent::needs_lane(&self.reply);
        let lane = if wants_lane { self.lane() } else { None };
        let unread = wants_lane && lane.is_none();
        let silence = Silence { quiet: self.quiet, eta_min: self.eta_min, lines: &self.lines };
        let Some(text) = silent::notice(&self.record.name, self.state, self.turn, &self.reply, lane.as_ref(), unread, Some(&silence)) else { return !(unread && self.state == State::Idle) };
        if self.cancelled.load(Ordering::SeqCst) {
            return true;
        }
        match self.ledger.claim_receipt(&self.record.name, &format!("silent:{}", self.turn), now_ms()) {
            Ok(true) => self.courier.notify(&self.parent, text, Delivery::Queued),
            Ok(false) => {}
            Err(error) => eprintln!("north fleet: silent notice to {} not sent: {}", self.parent, error.user_message()),
        }
        true
    }
}

struct SentPrompts {
    runtime: Arc<dyn Runtime>,
    journal: Mutex<Unaccepted>,
}

impl SentPrompts {
    fn journal(&self) -> MutexGuard<'_, Unaccepted> {
        self.journal.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn sent(&self, line: &str) {
        let Ok(frame) = serde_json::from_str::<Value>(line) else { return };
        if let Some(id) = self.runtime.sent_prompt(&frame) {
            self.journal().sent(id, line.trim_end().to_owned());
        }
    }
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

    /// The assistant texts the frame added.
    fn observe(&mut self, runtime: &dyn Runtime, frame: &Value) -> Vec<String> {
        let seen = self.transcript.entries.len();
        runtime.observe(frame, &mut self.transcript);
        let said: Vec<String> = self.transcript.entries[seen..].iter().filter(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.clone()).collect();
        if let Some(reply) = said.last() {
            self.reply = Some(reply.clone());
        }
        let surplus = self.transcript.entries.len().saturating_sub(KEPT_LINES);
        self.transcript.entries.drain(..surplus);
        said
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
            rolled_over: false,
            checkpoints: Checkpoints::default(),
            compacted: HashMap::new(),
            unaccepted,
            lanes: Arc::new(NoLanes),
            scope: Arc::new(NoScope),
            quiet: HashMap::new(),
            jobs: Arc::new(HeldLeases::default()),
            tickets: Arc::new(NoTickets),
            closing: false,
            usage: Arc::new(NoUsage),
            recording: Arc::new(NoRecording),
            verdicts: north_base::platform::finish_record::FinishRecords::here().0,
            reckon: Arc::new(Unreckoned),
        }
    }

    pub fn reading_jobs(mut self, jobs: Arc<dyn JobLeases>) -> Self {
        self.jobs = jobs;
        self
    }

    pub fn reading_tickets(mut self, tickets: Arc<dyn Tickets>) -> Self {
        self.tickets = tickets;
        self
    }

    pub fn reading_lanes(mut self, lanes: Arc<dyn Lanes + Send + Sync>) -> Self {
        self.lanes = lanes;
        self
    }

    pub fn writing_usage(mut self, usage: Arc<dyn UsageSink>) -> Self {
        self.usage = usage;
        self
    }

    pub fn reading_verdicts(mut self, verdicts: PathBuf) -> Self {
        self.verdicts = verdicts;
        self
    }

    pub fn recording_runs(mut self, recording: Arc<dyn RunRecording>) -> Self {
        self.recording = recording;
        self
    }

    pub fn reckoning_with(mut self, reckon: Arc<dyn Reckon>) -> Self {
        self.reckon = reckon;
        self
    }

    pub fn reading_scope(mut self, scope: Arc<dyn Scope>) -> Self {
        self.scope = scope;
        self
    }

    pub fn idling_by(mut self, policy: idle::Policy) -> Self {
        self.idle = policy;
        self
    }

    fn follow(&mut self, signal: &TurnSignal) {
        match signal {
            TurnSignal::Prompt { conversation, .. } => {
                self.quiet.remove(conversation);
                self.compacted.remove(conversation);
                self.finished.insert(conversation.clone(), false);
            }
            TurnSignal::Call { conversation, call, tokens } => {
                self.rolled_over = false;
                if let Some(calls) = self.compacted.get_mut(conversation) {
                    calls.insert(call.clone());
                }
                self.checkpoint(conversation, tokens.context());
            }
            TurnSignal::Compacted { conversation, .. } => {
                self.checkpoints.compacted(conversation);
                self.compacted.insert(conversation.clone(), HashSet::new());
            }
            TurnSignal::Ended { conversation, .. } => {
                let Some(reply) = self.views.get(conversation).and_then(|view| view.reply.clone()).filter(|reply| !reply.trim().is_empty()) else { return };
                if self.compacted.remove(conversation).is_some_and(|calls| calls.len() <= 1) && let Some(record) = find_session(&self.root, &self.host_dir, conversation) {
                    attribution::record(&record.name, "fleet", Cause::Harness(Harness::Compaction), "the turn compacted and ended in the next request, doing no more work");
                }
                self.finished.insert(conversation.clone(), retires(&reply));
            }
            _ => {}
        }
    }

    fn registered_wait(&self) -> bool {
        let waits = Waits::in_root(&self.root);
        host_sessions(&self.root, &self.host_dir).iter().any(|record| !waits.of(&record.name).is_empty())
    }

    fn checkpoint(&mut self, conversation: &str, context: u64) {
        if !self.checkpoints.due(conversation, context) || find_session(&self.root, &self.host_dir, conversation).is_none_or(|record| record.role() == Role::Proxy) {
            return;
        }
        self.checkpoints.ask(conversation, context);
        own(self.runtime.as_ref(), &checkpoint::question(context)).into_iter().flatten().for_each(|frame| self.courier.answer(&self.host_dir, frame));
    }

    /// The checkpoint's next frames once no turn is running: the worker's resume turn after its compaction, else the `/compact` its answer chose.
    fn checkpoint_frames(&mut self) -> Option<Vec<String>> {
        if self.turns.in_flight() {
            return None;
        }
        if self.checkpoints.take_resume() {
            return own(self.runtime.as_ref(), checkpoint::RESUME);
        }
        self.runtime.compact(Some(&self.checkpoints.take_ready()?)).map(|frame| vec![frame])
    }

    fn speak_up(&mut self, state: State, idle: Duration) {
        if !self.closing && self.turns.in_flight() {
            return;
        }
        let waits = Waits::in_root(&self.root);
        let cli_work = self.scope.cli_work() > 0;
        let conversations: Vec<String> = self.quiet.keys().cloned().collect();
        for conversation in conversations {
            let background = cli_work || self.turns.has_tasks(&conversation);
            let Some(quiet) = self.quiet.get_mut(&conversation) else { continue };
            quiet.settle();
            if quiet.checked || quiet.probe.is_some() || (state == State::Idle && (quiet.idled || !quiet.due(idle))) || background && !is_final(&quiet.reply) {
                continue;
            }
            let record = find_session(&self.root, &self.host_dir, &conversation);
            let Some(record) = record.filter(|record| record.phase() != Some(Phase::Stopped) && !(record.task_ref.is_some() && quiet.completed && wait::ended_on_a_wait(&quiet.reply)) && waits.of(&record.name).is_empty()) else { continue };
            let Some(parent) = record.parent.clone() else {
                quiet.checked = true;
                continue;
            };
            quiet.idled |= state == State::Idle;
            let tell = Tell { lanes: self.lanes.clone(), courier: self.courier.clone(), ledger: self.ledger.clone(), jobs: self.jobs.clone(), record, parent, state, turn: quiet.turn, reply: quiet.reply.clone(), cancelled: quiet.cancelled.clone(), quiet: quiet.since.elapsed(), eta_min: quiet.eta_min, lines: quiet.lines.clone() };
            quiet.probe = Some(std::thread::spawn(move || tell.run()));
        }
    }

    fn finish_probes(&mut self) {
        for quiet in self.quiet.values_mut() {
            if let Some(probe) = quiet.probe.take() {
                quiet.checked = probe.join().unwrap_or(true);
            }
        }
    }

    fn retire(&mut self, ending: Phase) -> bool {
        let sessions = host_sessions(&self.root, &self.host_dir);
        let retirable = !sessions.is_empty() && sessions.iter().all(|record| (record.parent.is_some() || record.task_ref.is_some()) && !parents_live(&self.root, &record.name) && self.landed_all(&record.name));
        if !retirable {
            self.finished.clear();
            return false;
        }
        let moved = Phases::open(&self.root).and_then(|mut phases| sessions.iter().filter(|record| record.live()).try_for_each(|record| phases.moved(record.launch(), ending, "its host ended after its final report").map(drop)));
        if let Err(error) = moved {
            eprintln!("north session-host: its sessions were not marked {}: {}", ending.name(), error.user_message());
        }
        true
    }

    fn landed_all(&self, session: &str) -> bool {
        match self.tickets.unlanded(session) {
            Ok(unlanded) => unlanded.is_empty(),
            Err(error) => {
                eprintln!("north session-host: the landing tickets of {session} were not read: {}", error.user_message());
                false
            }
        }
    }
}

fn own(runtime: &dyn Runtime, text: &str) -> Option<Vec<String>> {
    runtime.message(&Transcript::new(), &new_uuid().ok()?, &Sender::session_host().sign(text)).ok()
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
