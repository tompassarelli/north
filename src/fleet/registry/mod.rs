use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};

use super::books::{Digest, FleetBooks};
use super::claims::{self, Claims};
use super::gates;
use super::landings::{self, push_log};
use super::host::{Host, alive};
use super::model::{Account, ProviderName, Delivery, Effort, EntryKind, LaunchRequest, SessionRecord, Status, Transcript};
use super::prompt::{PromptInputs, StandingOrders, render, variant_sha};
use super::records::{PLACED, read_record, write_record};
use super::remote;
use super::roster::Roster;
use super::runtime::{DEFAULT_FOREGROUND_CAP_SECS, Instructions, Opening, Runtime, now_ms};
use super::runtimes::Runtimes;
pub use super::watch::TurnWatch;
use super::unaccepted::Unaccepted;
use super::transcript::{StreamTail, current_session_stream_index, ensure_session_streams, read_window, session_stream_path, stream_len, stream_path};

mod claiming;
mod depth;
mod efforts;
mod first_turn;
mod follower;
mod gate_commands;
mod home;
mod start_lock;
mod stopping;
pub use home::{orders_here, recorded_here};
mod letters;
mod parents;
mod past;
mod senders;
mod status;
mod stranding;
mod views;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

pub use follower::Follower;
pub use home::roster;
use home::{fleet_root, home_dir, state_dir};
pub use letters::{Opened, Sent};

const OPEN_WAIT: Duration = Duration::from_secs(30);
const OPEN_POLL: Duration = Duration::from_millis(100);
const SETTLE_WAIT: Duration = Duration::from_secs(15);
const SETTLE_POLL: Duration = Duration::from_millis(50);
const DEFAULT_IDLE_EXIT_MINUTES: u64 = 10;
const URGENT_PREFIX: &str = "[urgent message; your previous step was interrupted — answer this, then resume your task]";
const HOSTS: &str = ".hosts";

#[derive(Clone)]
pub struct Fleet {
    root: PathBuf,
    state: PathBuf,
    home: PathBuf,
    policy: PathBuf,
    runtimes: Runtimes,
    ledger: Arc<dyn FleetBooks>,
    claims: Arc<dyn Claims>,
}

impl Fleet {
    pub fn open(runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>) -> NorthResult<Self> {
        Self::rooted(home_dir()?, crate::pace::target::policy_path(), runtimes, ledger, claims)
    }

    fn rooted(home: PathBuf, policy: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>) -> NorthResult<Self> {
        let state = state_dir(&home);
        let root = fleet_root(&state);
        fs::create_dir_all(&root)?;
        Ok(Self { root, state, home, policy, runtimes, ledger, claims })
    }

    pub fn watch(&self, provider: &ProviderName, host_dir: &Path) -> NorthResult<TurnWatch> {
        Ok(TurnWatch::new(self.runtimes.get(provider)?, self.ledger.clone(), Arc::new(self.clone()), self.claims.clone(), self.root.clone(), host_dir.to_path_buf()))
    }

    pub fn orders(&self) -> StandingOrders {
        StandingOrders::in_state(&self.state)
    }

    pub fn roster(&self) -> Roster {
        self.runtimes.roster(&self.state, &self.home, &self.policy)
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.roster().accounts
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
            self.record(parent).map(drop).or_else(|error| self.outbox().relay().map_or(Err(error), |_| Ok(())))?;
        }
        let account = self.account(request)?;
        let runtime = self.runtimes.get(&request.provider)?;
        if let Some(model) = request.model.as_deref().filter(|model| !runtime.models().contains(model)) {
            return Err(NorthError::Usage(format!("{} has no model {model}; launch one of {}", request.provider.label(), runtime.models().join(", "))));
        }
        let effort = efforts::taken(runtime.as_ref(), request.effort)?;
        let conversation = runtime.assign_conversation()?;
        let verification = request.verification || std::env::var_os("NORTH_FLEET_TEST").is_some() || request.parent.as_deref().and_then(|parent| self.record(parent).ok()).is_some_and(|parent| parent.verification);
        let tasks = request.tasks();
        self.claim_before_start(request, &tasks, verification)?;
        fs::create_dir_all(&session_dir)?;
        let mut record = SessionRecord {
            verification,
            name: request.name.clone(),
            provider: request.provider.clone(),
            account: request.account.clone(),
            cwd: request.cwd.clone(),
            model: request.model.clone(),
            effort,
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
            task_ref: tasks.first().cloned(),
            extra_tasks: tasks.iter().skip(1).cloned().collect(),
            prompt_mode: request.prompt_mode,
            system_prompt_sha: None,
            placement: request.placement.clone(),
            machine: request.machine.clone(),
            capacity_need: request.capacity_need.clone(),
            last_sender: None,
            handoff_at: crate::fleet::handoff::threshold(&request.prompt),
        };
        self.start_claiming(&mut record, &account, conversation.as_deref())?;
        if let Some(task_ref) = &record.task_ref {
            self.ledger.join_task(task_ref, &request.name)?;
        }
        self.deliver_first_turn(&mut record, &account, &request.prompt)?;
        Ok(record)
    }

    pub fn revive(&self, name: &str, system_prompt: Option<PathBuf>) -> NorthResult<SessionRecord> {
        self.revive_placed(name, system_prompt, None, None)
    }

    /// Revive with a fresh placement override note and effort, which the restarted record keeps.
    pub fn revive_placed(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        self.record(name)?;
        let _start = self.start_lock(name)?;
        self.revive_held(name, system_prompt, placement, effort)
    }

    fn revive_held(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        let mut record = self.record(name)?;
        record.effort = efforts::revived(&record, self.runtime(&record)?.as_ref(), session_running(&record), effort)?;
        if system_prompt.is_some() {
            record.system_prompt = system_prompt;
        }
        if placement.is_some() {
            record.placement = placement;
        }
        if session_running(&record) {
            return Ok(record);
        }
        let conversation = record
            .conversation
            .clone()
            .ok_or_else(|| NorthError::State(format!("{name} never started a conversation")))?;
        let account = self.account(&record.relaunch_request())?;
        self.start(&mut record, &account, Opening::Resume(&conversation))?;
        Ok(record)
    }

    pub fn session(&self, name: &str) -> NorthResult<SessionRecord> {
        self.record(name)
    }

    pub fn running(&self, record: &SessionRecord) -> bool {
        session_running(record)
    }

    pub fn send(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        let record = self.revive(name, None)?;
        let transcript = self.recent(self.runtime(&record)?.as_ref(), &record)?;
        match delivery {
            Delivery::Now if transcript.status != Status::Idle => self.send_urgent(&record, &transcript, text),
            _ => self.send_text(&record, &transcript, text),
        }
    }

    pub fn interrupt(&self, name: &str) -> NorthResult<()> {
        let record = self.record(name)?;
        let runtime = self.runtime(&record)?;
        let transcript = self.recent(runtime.as_ref(), &record)?;
        Host::new(record.host_dir.clone()).send(&runtime.interrupt(&transcript)?)
    }

    pub fn observe_landings(&self) -> NorthResult<()> {
        let log = push_log();
        for record in self.sessions()? {
            landings::observe(self.ledger.as_ref(), &record, &log)?;
        }
        Ok(())
    }

    pub fn digest(&self, since_ms: u64) -> NorthResult<Digest> {
        Ok(Digest { gates: gates::decisions(self.ledger.gates(since_ms)?), spend: self.ledger.spend()?, landed: self.ledger.landed_since(since_ms)? })
    }

    pub fn status(&self, record: &SessionRecord) -> Status {
        if record.host_dir.as_os_str().is_empty() {
            return status::settled(record, Status::Starting);
        }
        self.status_from_index(record).unwrap_or(Status::Exited)
    }

    pub fn follow(&self, name: &str) -> NorthResult<Follower> {
        self.follow_with_history(name, true)
    }

    pub fn follow_recent(&self, name: &str) -> NorthResult<Follower> {
        self.follow_with_history(name, false)
    }

    fn follow_with_history(&self, name: &str, full_history: bool) -> NorthResult<Follower> {
        let record = self.record(name)?;
        let (path, indexed_to) = self.stream_index(&record)?;
        Follower::open(self.runtime(&record)?, record, self.root.join(name), path, indexed_to, full_history)
    }

    pub(super) fn recent(&self, runtime: &dyn Runtime, record: &SessionRecord) -> NorthResult<Transcript> {
        let (path, _) = self.stream_index(record)?;
        let mut transcript = seeded(record);
        for frame in read_frames(&path)? {
            runtime.observe(&frame, &mut transcript);
        }
        if let Some(status) = status::latest_turn_status(&path, 0, record.conversation.as_deref(), &|frame| runtime.signals(frame))? {
            transcript.status = status;
        }
        Ok(transcript)
    }

    pub(super) fn recent_frames(&self, record: &SessionRecord) -> NorthResult<Vec<serde_json::Value>> {
        let (path, _) = self.stream_index(record)?;
        read_frames(&path)
    }

    fn status_from_index(&self, record: &SessionRecord) -> NorthResult<Status> {
        let (path, _) = self.stream_index(record)?;
        let observed = self.runtime(record).and_then(|runtime| {
            status::latest_turn_status(&path, 0, record.conversation.as_deref(), &|frame| runtime.signals(frame))
        })?.unwrap_or(Status::Starting);
        Ok(status::settled(record, observed))
    }

    fn stream_index(&self, record: &SessionRecord) -> NorthResult<(PathBuf, u64)> {
        if record.host_dir.as_os_str().is_empty() {
            return Err(NorthError::State(format!("{} has no host stream", record.name)));
        }
        if let Some(indexed_to) = current_session_stream_index(&record.host_dir, &record.name) {
            return Ok((session_stream_path(&record.host_dir, &record.name), indexed_to));
        }
        let records: Vec<SessionRecord> = self.sessions()?.into_iter().filter(|session| session.host_dir == record.host_dir).collect();
        let runtime = self.runtime(record)?;
        let indexed_to = ensure_session_streams(&record.host_dir, &records, |frame| runtime.frame_conversation(frame).map(str::to_owned))?;
        Ok((session_stream_path(&record.host_dir, &record.name), indexed_to))
    }

    fn start(&self, record: &mut SessionRecord, account: &Account, opening: Opening) -> NorthResult<()> {
        let runtime = self.runtime(record)?;
        let request = record.relaunch_request();
        let (instructions, sha) = self.instructions(record)?;
        let changed = record.system_prompt_sha.as_deref() != Some(sha.as_str());
        record.system_prompt_sha = Some(sha);
        let spec = remote::placed(runtime.host(account, &request, opening, &instructions)?, record)?;
        let host_dir = if spec.shared { self.root.join(HOSTS).join(&spec.key) } else { self.root.join(&record.name) };
        let host = Host::new(host_dir.clone());
        let started = !host.running();
        if started {
            host.start(&spec, &record.provider, idle_exit(record))?;
            host.send(&runtime.handshake())?;
        }
        if record.host_dir != host_dir {
            let _ = fs::remove_file(session_stream_path(&host_dir, &record.name));
            record.stream_offset = stream_len(&host_dir);
            record.host_dir = host_dir.clone();
        }
        record.host_pid = host.pid();
        record.stopped = false;
        write_record(&self.root.join(&record.name), record)?;
        self.watch_over(record)?;
        let frames = runtime.open(&record.name, &request, opening, Instructions { text: &instructions, changed })?;
        if !frames.is_empty() {
            let opened_from = stream_len(&host_dir);
            host.send(&frames)?;
            record.conversation = Some(await_conversation(runtime.as_ref(), record, opened_from)?);
            write_record(&self.root.join(&record.name), record)?;
        }
        if started {
            host.send(&Unaccepted::to_replay(&host_dir))?;
        }
        remote::leave_record(record, &self.root.join(PLACED));
        claims::hold_for(self.claims.as_ref(), record);
        Ok(())
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
        self.runtimes.account(&self.state, &self.home, &self.policy, request)
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

fn read_frames(path: &Path) -> NorthResult<Vec<serde_json::Value>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let bytes = read_window(&mut file, 0, u64::MAX)?;
    let complete = bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |index| index + 1);
    Ok(bytes[..complete]
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect())
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

pub fn urgent_prefixed(text: &str) -> String {
    format!("{URGENT_PREFIX}\n{text}")
}

pub fn urgent_text(text: &str) -> Option<&str> {
    text.strip_prefix(URGENT_PREFIX).map(|rest| rest.trim_start_matches('\n'))
}
