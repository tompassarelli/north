use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};

use super::books::{Books, Digest};
use super::gates::{Closure, Decision, Gate, GateBook, GateRequest, settle};
use super::host::{Host, alive};
use super::model::{Account, ProviderName, Delivery, EntryKind, LaunchRequest, SessionRecord, Status, Transcript};
use super::prompt::{PromptInputs, StandingOrders, render, variant_sha};
use super::records::{ARCHIVE, read_record, write_record};
use super::runtime::{DEFAULT_FOREGROUND_CAP_SECS, Instructions, Opening, Runtime, Runtimes, now_ms};
use super::turns::RunSink;
pub use super::watch::TurnWatch;
use super::transcript::{StreamTail, stream_len, stream_path};

const OPEN_WAIT: Duration = Duration::from_secs(30);
const OPEN_POLL: Duration = Duration::from_millis(100);
const SETTLE_WAIT: Duration = Duration::from_secs(15);
const SETTLE_POLL: Duration = Duration::from_millis(50);
const DEFAULT_IDLE_EXIT_MINUTES: u64 = 10;
const RECENT_WINDOW: u64 = 1 << 20;
const URGENT_PREFIX: &str = "[urgent message; your previous step was interrupted — answer this, then resume your task]";
const HOSTS: &str = ".hosts";

pub struct Fleet {
    root: PathBuf,
    state: PathBuf,
    home: PathBuf,
    runtimes: Runtimes,
    sink: Arc<dyn RunSink>,
    gates: Arc<dyn GateBook>,
    books: Arc<dyn Books>,
}

impl Fleet {
    pub fn open<L: RunSink + GateBook + Books + 'static>(runtimes: Runtimes, ledger: Arc<L>) -> NorthResult<Self> {
        let home = home_dir()?;
        let state = state_dir(&home);
        let root = state.join("fleet");
        fs::create_dir_all(&root)?;
        Ok(Self { root, state, home, runtimes, sink: ledger.clone(), gates: ledger.clone(), books: ledger })
    }

    pub fn watch(&self, provider: &ProviderName, host_dir: &Path) -> NorthResult<TurnWatch> {
        Ok(TurnWatch::new(self.runtimes.get(provider)?, self.sink.clone(), self.root.clone(), host_dir.to_path_buf()))
    }

    pub fn orders(&self) -> StandingOrders {
        StandingOrders::in_state(&self.state)
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.runtimes.accounts(&self.state, &self.home)
    }

    pub fn providers(&self) -> Vec<ProviderName> {
        self.runtimes.providers()
    }

    pub fn sessions(&self) -> NorthResult<Vec<SessionRecord>> {
        let mut sessions: Vec<SessionRecord> = fs::read_dir(&self.root)?
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|entry| read_record(&entry.path()).ok())
            .collect();
        sessions.sort_by_key(|record| record.created_ms);
        Ok(sessions)
    }

    pub fn launch(&self, request: &LaunchRequest) -> NorthResult<SessionRecord> {
        let session_dir = self.session_dir(&request.name)?;
        if session_dir.exists() {
            return Err(NorthError::Usage(format!("a session named {} already exists", request.name)));
        }
        if let Some(parent) = &request.parent {
            self.record(parent)?;
        }
        let account = self.account(request)?;
        let runtime = self.runtimes.get(&request.provider)?;
        let conversation = runtime.assign_conversation()?;
        fs::create_dir_all(&session_dir)?;
        let mut record = SessionRecord {
            name: request.name.clone(),
            provider: request.provider.clone(),
            account: request.account.clone(),
            cwd: request.cwd.clone(),
            model: request.model.clone(),
            created_ms: now_ms(),
            host_pid: None,
            conversation: conversation.clone(),
            host_dir: PathBuf::new(),
            stream_offset: 0,
            stopped: false,
            system_prompt: request.system_prompt.clone(),
            foreground_cap_secs: request.foreground_cap_secs,
            idle_exit_minutes: request.idle_exit_minutes,
            parent: request.parent.clone(),
            task_ref: request.task_ref.clone(),
            prompt_mode: request.prompt_mode,
            system_prompt_sha: None,
        };
        self.start(&mut record, &account, Opening::Fresh(conversation.as_deref()))?;
        if let Some(task_ref) = &request.task_ref {
            self.sink.join_task(task_ref, &request.name)?;
        }
        self.send_text(&record, &recent(runtime.as_ref(), &record)?, &request.prompt)?;
        Ok(record)
    }

    pub fn revive(&self, name: &str, system_prompt: Option<PathBuf>) -> NorthResult<SessionRecord> {
        let mut record = self.record(name)?;
        if system_prompt.is_some() {
            record.system_prompt = system_prompt;
        }
        if session_running(&record) {
            return Ok(record);
        }
        let conversation = record
            .conversation
            .clone()
            .ok_or_else(|| NorthError::State(format!("{name} never started a conversation")))?;
        let account = self.account(&relaunch_request(&record))?;
        self.start(&mut record, &account, Opening::Resume(&conversation))?;
        Ok(record)
    }

    pub fn send(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        let record = self.revive(name, None)?;
        let transcript = recent(self.runtime(&record)?.as_ref(), &record)?;
        match delivery {
            Delivery::Now if transcript.status != Status::Idle => self.send_urgent(&record, &transcript, text),
            _ => self.send_text(&record, &transcript, text),
        }
    }

    pub fn ask(&self, name: &str, question: &str, timeout: Duration) -> NorthResult<Option<String>> {
        let record = self.revive(name, None)?;
        let asked_from = stream_len(&record.host_dir);
        self.send(name, question, Delivery::Now)?;
        await_reply(self.runtime(&record)?.as_ref(), &record, asked_from, question, timeout)
    }

    pub fn interrupt(&self, name: &str) -> NorthResult<()> {
        let record = self.record(name)?;
        let runtime = self.runtime(&record)?;
        let transcript = recent(runtime.as_ref(), &record)?;
        Host::new(record.host_dir.clone()).send(&runtime.interrupt(&transcript)?)
    }

    pub fn stop(&self, name: &str) -> NorthResult<()> {
        let mut record = self.record(name)?;
        let host = Host::new(record.host_dir.clone());
        if session_running(&record) {
            let runtime = self.runtime(&record)?;
            let transcript = recent(runtime.as_ref(), &record)?;
            let _ = host.send(&runtime.close(&transcript));
        }
        record.stopped = true;
        write_record(&self.root.join(name), &record)?;
        if !self.host_in_use(&record)? {
            host.terminate();
        }
        Ok(())
    }

    pub fn forget(&self, name: &str) -> NorthResult<()> {
        self.stop(name)?;
        let archive = self.root.join(ARCHIVE);
        fs::create_dir_all(&archive)?;
        fs::rename(self.root.join(name), archive.join(format!("{name}-{}", now_ms())))?;
        Ok(())
    }

    pub fn open_gate(&self, request: &GateRequest) -> NorthResult<i64> {
        request.validate()?;
        self.record(&request.session)?;
        self.gates.open_gate(request, now_ms())
    }

    pub fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        self.gates.gate(id)
    }

    pub fn decide(&self, id: i64, decision: Decision) -> NorthResult<Closure> {
        settle(self.gates.as_ref(), id, decision, now_ms(), |session, text| self.send(session, text, Delivery::Now))
    }

    pub fn digest(&self, since_ms: u64) -> NorthResult<Digest> {
        Ok(Digest { gates: self.gates.gates(since_ms)?, spend: self.books.spend()?, landed: self.books.landed_since(since_ms)? })
    }

    pub fn status(&self, record: &SessionRecord) -> Status {
        let observed = self.runtime(record).and_then(|runtime| recent(runtime.as_ref(), record)).map_or(Status::Exited, |transcript| transcript.status);
        settled_status(record, observed)
    }

    pub fn follow(&self, name: &str) -> NorthResult<Follower> {
        let record = self.record(name)?;
        Follower::open(self.runtime(&record)?, record, self.root.join(name))
    }

    fn start(&self, record: &mut SessionRecord, account: &Account, opening: Opening) -> NorthResult<()> {
        let runtime = self.runtime(record)?;
        let request = relaunch_request(record);
        let (instructions, sha) = self.instructions(record)?;
        let changed = record.system_prompt_sha.as_deref() != Some(sha.as_str());
        record.system_prompt_sha = Some(sha);
        let spec = runtime.host(account, &request, opening, &instructions);
        let host_dir = if spec.shared { self.root.join(HOSTS).join(&spec.key) } else { self.root.join(&record.name) };
        let host = Host::new(host_dir.clone());
        if !host.running() {
            host.start(&spec, &record.provider, idle_exit(record))?;
            host.send(&runtime.handshake())?;
        }
        if record.host_dir != host_dir {
            record.stream_offset = stream_len(&host_dir);
            record.host_dir = host_dir.clone();
        }
        record.host_pid = host.pid();
        record.stopped = false;
        write_record(&self.root.join(&record.name), record)?;
        let frames = runtime.open(&record.name, &request, opening, Instructions { text: &instructions, changed })?;
        if frames.is_empty() {
            return Ok(());
        }
        let opened_from = stream_len(&host_dir);
        host.send(&frames)?;
        record.conversation = Some(await_conversation(runtime.as_ref(), record, opened_from)?);
        write_record(&self.root.join(&record.name), record)
    }

    fn instructions(&self, record: &SessionRecord) -> NorthResult<(String, String)> {
        let orders = self.orders().read()?;
        let extra = record.system_prompt.as_deref().map(fs::read_to_string).transpose()?;
        let inputs = PromptInputs {
            session: &record.name,
            mode: record.prompt_mode,
            orders: &orders,
            foreground_cap_secs: record.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS),
            extra: extra.as_deref(),
        };
        Ok((render(&inputs), variant_sha(&inputs)))
    }

    fn send_text(&self, record: &SessionRecord, transcript: &Transcript, text: &str) -> NorthResult<()> {
        Host::new(record.host_dir.clone()).send(&self.runtime(record)?.message(transcript, text)?)
    }

    fn send_urgent(&self, record: &SessionRecord, transcript: &Transcript, text: &str) -> NorthResult<()> {
        let runtime = self.runtime(record)?;
        let interrupted_from = stream_len(&record.host_dir);
        Host::new(record.host_dir.clone()).send(&runtime.interrupt(transcript)?)?;
        let settled = await_settled(runtime.as_ref(), record, transcript.clone(), interrupted_from)?;
        self.send_text(record, &settled, &urgent_prefixed(text))
    }

    fn host_in_use(&self, stopping: &SessionRecord) -> NorthResult<bool> {
        Ok(self.sessions()?.iter().any(|other| {
            other.name != stopping.name && other.host_dir == stopping.host_dir && session_running(other)
        }))
    }

    fn account(&self, request: &LaunchRequest) -> NorthResult<Account> {
        self.accounts()
            .into_iter()
            .find(|account| account.provider == request.provider && account.label == request.account)
            .ok_or_else(|| NorthError::Usage(format!("no {} account named {}", request.provider.label(), request.account)))
    }

    fn runtime(&self, record: &SessionRecord) -> NorthResult<Arc<dyn Runtime>> {
        self.runtimes.get(&record.provider)
    }

    fn record(&self, name: &str) -> NorthResult<SessionRecord> {
        read_record(&self.session_dir(name)?).map_err(|_| NorthError::Usage(format!("no session named {name}")))
    }

    fn session_dir(&self, name: &str) -> NorthResult<PathBuf> {
        let valid = !name.is_empty() && !name.starts_with('.') && !name.contains('/');
        if !valid {
            return Err(NorthError::Usage(format!("invalid session name {name:?}")));
        }
        Ok(self.root.join(name))
    }
}

pub struct Follower {
    runtime: Arc<dyn Runtime>,
    record: SessionRecord,
    session_dir: PathBuf,
    tail: StreamTail,
    transcript: Transcript,
}

impl Follower {
    fn open(runtime: Arc<dyn Runtime>, record: SessionRecord, session_dir: PathBuf) -> NorthResult<Self> {
        let tail = StreamTail::open(stream_path(&record.host_dir), record.stream_offset);
        let transcript = seeded(&record);
        let mut follower = Self { runtime, record, session_dir, tail, transcript };
        follower.poll()?;
        Ok(follower)
    }

    pub fn poll(&mut self) -> NorthResult<bool> {
        let before = (self.transcript.entries.len(), self.transcript.status);
        self.refresh_record();
        let frames = self.tail.frames()?;
        if !frames.is_empty() {
            self.transcript.last_activity_ms = Some(now_ms());
        }
        for frame in &frames {
            if let Some(quota) = self.runtime.quota(frame) {
                self.transcript.quota = Some(quota);
            }
            project(self.runtime.as_ref(), frame, &mut self.transcript);
        }
        self.persist_conversation()?;
        self.transcript.status = settled_status(&self.record, self.transcript.status);
        Ok(!frames.is_empty() || before != (self.transcript.entries.len(), self.transcript.status))
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn record(&self) -> &SessionRecord {
        &self.record
    }

    fn refresh_record(&mut self) {
        if let Ok(record) = read_record(&self.session_dir) {
            self.record = record;
        }
    }

    fn persist_conversation(&mut self) -> NorthResult<()> {
        let observed = self.transcript.conversation.clone();
        if observed.is_none() || observed == self.record.conversation {
            return Ok(());
        }
        self.record.conversation = observed;
        write_record(&self.session_dir, &self.record)
    }
}

fn project(runtime: &dyn Runtime, frame: &serde_json::Value, transcript: &mut Transcript) {
    runtime.observe(frame, transcript);
    for signal in runtime.signals(frame) {
        if transcript.conversation.as_deref() != Some(signal.conversation()) {
            continue;
        }
        if let Some(turn) = transcript.tracker.observe(signal, now_ms()) {
            transcript.turns.push(turn);
        }
    }
}

fn settled_status(record: &SessionRecord, observed: Status) -> Status {
    match (session_running(record), record.stopped) {
        (true, _) if matches!(observed, Status::Exited | Status::Parked) => Status::Idle,
        (true, _) => observed,
        (false, true) => Status::Exited,
        (false, false) => Status::Parked,
    }
}

fn idle_exit(record: &SessionRecord) -> Option<Duration> {
    let minutes = record.idle_exit_minutes.unwrap_or(DEFAULT_IDLE_EXIT_MINUTES);
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}

fn session_running(record: &SessionRecord) -> bool {
    !record.stopped && record.host_pid.is_some_and(alive)
}

fn seeded(record: &SessionRecord) -> Transcript {
    let mut transcript = Transcript::new();
    transcript.name = record.name.clone();
    transcript.conversation = record.conversation.clone();
    transcript
}

fn recent(runtime: &dyn Runtime, record: &SessionRecord) -> NorthResult<Transcript> {
    let window_start = stream_len(&record.host_dir).saturating_sub(RECENT_WINDOW).max(record.stream_offset);
    let mut tail = StreamTail::open(stream_path(&record.host_dir), window_start);
    let mut transcript = seeded(record);
    for frame in tail.frames()? {
        runtime.observe(&frame, &mut transcript);
    }
    Ok(transcript)
}

fn await_settled(runtime: &dyn Runtime, record: &SessionRecord, mut transcript: Transcript, offset: u64) -> NorthResult<Transcript> {
    let mut tail = StreamTail::open(stream_path(&record.host_dir), offset);
    let deadline = Instant::now() + SETTLE_WAIT;
    while transcript.status == Status::Working && Instant::now() < deadline {
        std::thread::sleep(SETTLE_POLL);
        for frame in tail.frames()? {
            runtime.observe(&frame, &mut transcript);
        }
    }
    Ok(transcript)
}

fn await_reply(runtime: &dyn Runtime, record: &SessionRecord, offset: u64, question: &str, timeout: Duration) -> NorthResult<Option<String>> {
    let mut tail = StreamTail::open(stream_path(&record.host_dir), offset);
    let mut transcript = seeded(record);
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        for frame in tail.frames()? {
            runtime.observe(&frame, &mut transcript);
        }
        if let Some(reply) = reply_to(&transcript, question) {
            return Ok(Some(reply));
        }
        std::thread::sleep(SETTLE_POLL);
    }
    Ok(None)
}

fn reply_to(transcript: &Transcript, question: &str) -> Option<String> {
    let asked = transcript.entries.iter().position(|entry| entry.kind == EntryKind::User && entry.text.contains(question))?;
    transcript.entries[asked..].iter().find(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.clone())
}

fn await_conversation(runtime: &dyn Runtime, record: &SessionRecord, offset: u64) -> NorthResult<String> {
    let mut tail = StreamTail::open(stream_path(&record.host_dir), offset);
    let mut transcript = seeded(record);
    transcript.conversation = None;
    let deadline = Instant::now() + OPEN_WAIT;
    loop {
        for frame in tail.frames()? {
            runtime.observe(&frame, &mut transcript);
        }
        if let Some(conversation) = transcript.conversation.take() {
            return Ok(conversation);
        }
        if Instant::now() > deadline || !record.host_pid.is_some_and(alive) {
            return Err(NorthError::State(format!("{} did not open a conversation", record.name)));
        }
        std::thread::sleep(OPEN_POLL);
    }
}

fn relaunch_request(record: &SessionRecord) -> LaunchRequest {
    LaunchRequest {
        name: record.name.clone(),
        provider: record.provider.clone(),
        account: record.account.clone(),
        cwd: record.cwd.clone(),
        model: record.model.clone(),
        prompt: String::new(),
        system_prompt: record.system_prompt.clone(),
        foreground_cap_secs: record.foreground_cap_secs,
        prompt_mode: record.prompt_mode,
        idle_exit_minutes: record.idle_exit_minutes,
        parent: record.parent.clone(),
        task_ref: record.task_ref.clone(),
    }
}

pub fn urgent_prefixed(text: &str) -> String {
    format!("{URGENT_PREFIX}\n{text}")
}

pub fn urgent_text(text: &str) -> Option<&str> {
    text.strip_prefix(URGENT_PREFIX).map(|rest| rest.trim_start_matches('\n'))
}

fn home_dir() -> NorthResult<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| NorthError::Configuration("HOME is not set".into()))
}

fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/north")
}
