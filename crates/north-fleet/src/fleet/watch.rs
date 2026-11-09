use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;

use crate::fleet::session_host::{FrameWatch, Rest, SentLog};

use north_base::lifecycle::State as _;
use north_base::platform::attribution::{self, Cause, Harness};

use super::books::WatchBooks;
use super::claims::{self, Claims};
use super::host::Host;
use super::detached::{Detached, NoScope, Scope};
use super::gates::Question;
use super::checkpoint::{self, Checkpoints};
use super::idle::{self, Step};
use super::jobs::{HeldLeases, JobLeases};
use super::landings::{self, push_log};
use super::model::{Delivery, Entry, EntryKind, Machine, Phase, Phases, Role, SessionRecord, Transcript};
use super::prompt::is_deepened;
use super::promises::{Book, Promise, promised, resolver};
use super::receipts::{Courier, Report, deliver_report, is_final, retires};
use super::records::{find_session, host_sessions, parents_live};
use super::silent::{self, Quiet, Silence, State};
use super::run_recording::{NoRecording, RunRecording};
use super::runtime::{Runtime, new_uuid, now_ms};
use super::sender::Sender;
use super::stranded::{Checkout, Lane, Lanes, Worktree};
use super::tickets::{NoTickets, Tickets};
use super::turns::{RunRecord, Turn, TurnExit, TurnSignal, Turns};
use super::unaccepted::Unaccepted;
use super::usage_sink::{NoUsage, UsageSink};
use super::wait;
use super::waits::Waits;

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
    compacted: HashSet<String>,
    unaccepted: Arc<SentPrompts>,
    lanes: Arc<dyn Lanes + Send + Sync>,
    scope: Arc<dyn Scope>,
    detached: Vec<Detached>,
    quiet: HashMap<String, Quiet>,
    jobs: Arc<dyn JobLeases>,
    tickets: Arc<dyn Tickets>,
    closing: bool,
    usage: Arc<dyn UsageSink>,
    recording: Arc<dyn RunRecording>,
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
}

impl SentLog for SentPrompts {
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
            compacted: HashSet::new(),
            unaccepted,
            lanes: Arc::new(NoLanes),
            scope: Arc::new(NoScope),
            detached: Vec::new(),
            quiet: HashMap::new(),
            jobs: Arc::new(HeldLeases::default()),
            tickets: Arc::new(NoTickets),
            closing: false,
            usage: Arc::new(NoUsage),
            recording: Arc::new(NoRecording),
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

    pub fn recording_runs(mut self, recording: Arc<dyn RunRecording>) -> Self {
        self.recording = recording;
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
            TurnSignal::Call { conversation, tokens, .. } => {
                self.rolled_over = false;
                self.checkpoint(conversation, tokens.context());
            }
            TurnSignal::Compacted { conversation, .. } => {
                self.checkpoints.compacted(conversation);
                self.compacted.insert(conversation.clone());
            }
            TurnSignal::Ended { conversation, .. } => {
                let Some(reply) = self.views.get(conversation).and_then(|view| view.reply.clone()).filter(|reply| !reply.trim().is_empty()) else { return };
                if self.compacted.remove(conversation) && !attribution::has_opener(&reply) && let Some(record) = find_session(&self.root, &self.host_dir, conversation) {
                    attribution::record(&record.name, "fleet", Cause::Harness(Harness::Compaction), "the turn compacted and ended without a Done, Not done, Split or Blocked opener");
                }
                self.finished.insert(conversation.clone(), retires(&reply));
            }
            _ => {}
        }
    }

    fn register_detached(&self) {
        let waits = Waits::in_root(&self.root);
        for record in host_sessions(&self.root, &self.host_dir) {
            for run in &self.detached {
                if !run.is_a_waited_run() {
                    continue;
                }
                if let Err(error) = waits.register(&record.name, run.run(), &record.cwd, Some(&run.command)) {
                    eprintln!("north session-host: {} holds a detached run without its wait on {}: {}", record.name, run.run(), error.user_message());
                }
            }
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

    fn record(&mut self, mut turn: Turn) {
        let session = find_session(&self.root, &self.host_dir, &turn.conversation);
        identify(&mut turn, session.as_ref());
        let run = RunRecord {
            session: session.as_ref().map_or_else(|| turn.conversation.clone(), |record| record.name.clone()),
            task_ref: session.as_ref().and_then(|record| record.task_ref.clone()),
            provider: self.runtime.provider().label().to_owned(),
            account: session.as_ref().map(|record| Host::served_by(&self.host_dir).unwrap_or_else(|| record.account.clone())),
            system_prompt_sha: session.as_ref().and_then(|record| record.system_prompt_sha.clone()),
            deepen: turn.prompt.as_deref().is_some_and(is_deepened),
            turn,
        };
        let number = self.ledger.record_run(&run).map_err(|error| eprintln!("north ledger: {}", error.user_message())).ok();
        let reply = self.views.get(&run.turn.conversation).and_then(|view| view.reply.clone()).unwrap_or_default();
        let lines = self.views.get(&run.turn.conversation).map(|view| view.transcript.entries.clone()).unwrap_or_default();
        let Some(record) = session else { return };
        self.observe_landing(&record);
        if let Some(number) = number {
            if matches!(run.turn.exit, TurnExit::Completed | TurnExit::Interrupted) {
                self.quiet.insert(run.turn.conversation.clone(), Quiet::after(number, reply, run.turn.exit == TurnExit::Completed, record.eta_min, self.idle.eta_minute, lines));
            }
            self.report(&record, &run.turn, number);
        }
    }

    fn report(&mut self, record: &SessionRecord, turn: &Turn, number: u64) {
        let view = self.views.remove(&turn.conversation);
        let Some(parent) = record.parent.as_deref() else { return };
        let (reply, lines) = view.map(|view| (view.reply, view.transcript.entries)).unwrap_or_default();
        let landed = landings::landed_between(record, &landings::pushes_of(record, &push_log()), turn.started_ms, turn.ended_ms);
        if turn.exit == TurnExit::Completed {
            self.keep_promises(record, turn, number, reply.as_deref().unwrap_or_default());
        }
        let waiting = self.tickets.unlanded(&record.name).is_ok_and(|tickets| !tickets.is_empty()) || !Waits::in_root(&self.root).of(&record.name).is_empty();
        let report = Report { child: record.name.clone(), turn: number, exit: turn.exit, landed, reply, waiting };
        if let Err(error) = deliver_report(self.ledger.as_ref(), self.ledger.as_ref(), self.courier.as_ref(), parent, &report, &lines, now_ms()) {
            eprintln!("north fleet: receipt to {parent} not sent: {}", error.user_message());
        }
    }

    fn keep_promises(&self, record: &SessionRecord, turn: &Turn, number: u64, reply: &str) {
        let mut promises = Vec::new();
        let search = std::env::var_os("PATH").unwrap_or_default();
        for claim in promised(reply, &resolver(&record.cwd, &search)) {
            match claim.command {
                Some(command) => promises.push(Promise { session: record.name.clone(), turn: number, line: claim.line, command, when: claim.when, said_ms: turn.ended_ms, cwd: record.cwd.clone() }),
                None => eprintln!("north fleet: {}'s line {:?} names a time but no backticked command whose program resolves, so it holds no promise to check", record.name, claim.line),
            }
        }
        if let Err(error) = Book::at(&self.root).keep(&promises) {
            eprintln!("north fleet: {}'s promises were not kept for the tick to check: {}", record.name, error.user_message());
        }
    }

    fn relay(&self, question: Question) {
        let Some(record) = find_session(&self.root, &self.host_dir, &question.conversation) else { return };
        for ask in &question.asks {
            self.courier.ask(ask.gate(&record.name));
        }
        self.courier.answer(&self.host_dir, question.reply);
    }

    fn record_usage(&self, frame: &Value) {
        if self.runtime.quota(frame).is_none() {
            return;
        }
        let Some(session) = self.runtime.frame_conversation(frame).and_then(|conversation| find_session(&self.root, &self.host_dir, conversation)) else { return };
        self.usage.observe(self.runtime.provider().label(), &session.account, frame);
    }

    fn observe_landing(&self, record: &SessionRecord) {
        if let Err(error) = landings::observe_among(self.ledger.as_ref(), record, &landings::pushes_of(record, &push_log())) {
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
        self.record_usage(&frame);
        let signals = self.runtime.signals(&frame);
        for signal in &signals {
            self.views.entry(signal.conversation().to_owned()).or_insert_with(|| View::of(signal.conversation()));
        }
        let answers: Vec<_> = self.views.iter_mut().map(|(conversation, view)| (conversation.clone(), view.observe(self.runtime.as_ref(), &frame))).filter(|(_, said)| !said.is_empty()).collect();
        let answers: Vec<_> = answers.into_iter().filter(|(conversation, _)| self.checkpoints.awaiting(conversation)).collect();
        for (conversation, said) in answers {
            let model = find_session(&self.root, &self.host_dir, &conversation).and_then(|record| record.model);
            self.checkpoints.answered(&conversation, &said, model.as_deref());
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

    fn soonest_rest(&self) -> Option<Duration> {
        Some(self.idle.registered_park_after)
    }

    fn rest(&mut self, idle: Duration, park_after: Option<Duration>) -> Rest {
        let live = self.scope.detached();
        self.detached = live;
        self.register_detached();
        let finished = !self.finished.is_empty() && self.finished.values().all(|finished| *finished);
        let registered = self.registered_wait();
        let session = idle::Session { idle, in_flight: self.turns.in_flight(), turn_open: self.turns.turn_open(), context: self.turns.context(), rolled_over: self.rolled_over, finished, registered };
        let step = self.idle.step(session, park_after);
        match step {
            Step::Park => {
                self.speak_up(State::Parked, idle);
            }
            _ if self.quiet.values().any(|quiet| quiet.due(idle)) => self.speak_up(State::Idle, idle),
            _ => {}
        }
        match step {
            Step::Wait => self.checkpoint_frames().map_or(Rest::Wait, Rest::Send),
            Step::Park => Rest::End,
            Step::Compact => {
                self.rolled_over = true;
                self.runtime.compact(None).map_or(Rest::Wait, |frame| Rest::Send(vec![frame]))
            }
            Step::Exit if self.retire(Phase::IdleExited) => {
                self.speak_up(State::Exited, idle);
                Rest::End
            }
            Step::Exit => self.rest(idle, park_after),
        }
    }

    fn close(&mut self) {
        self.closing = true;
        for turn in self.turns.abandon(now_ms()) {
            self.record(turn);
        }
        let ended: HashSet<String> = host_sessions(&self.root, &self.host_dir).into_iter().filter_map(|record| record.conversation).collect();
        for conversation in &ended {
            self.recording.ended(conversation);
        }
        let retired = !self.finished.is_empty() && self.finished.values().all(|finished| *finished) && self.unaccepted.journal().is_empty() && self.retire(Phase::HostEnded);
        let waits = Waits::in_root(&self.root);
        for record in host_sessions(&self.root, &self.host_dir).into_iter().filter(|record| record.ended() || waits.of(&record.name).is_empty()) {
            claims::release_for(self.claims.as_ref(), &record);
        }
        self.finish_probes();
        self.speak_up(if retired { State::Exited } else { State::Parked }, Duration::ZERO);
        self.finish_probes();
    }
}

fn own(runtime: &dyn Runtime, text: &str) -> Option<Vec<String>> {
    runtime.message(&Transcript::new(), &new_uuid().ok()?, &Sender::session_host().sign(text)).ok()
}

/// A turn's model and effort as observed, else as its session was launched.
fn identify(turn: &mut Turn, session: Option<&SessionRecord>) {
    if turn.model.is_none() {
        turn.model = session.and_then(|record| record.model.clone());
    }
    if turn.effort.is_none() {
        turn.effort = session.and_then(|record| record.effort).map(|effort| effort.label().to_owned());
    }
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
