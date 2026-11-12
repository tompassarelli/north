use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};

use super::books::{Digest, FleetBooks, Revision, SessionSpend};
use super::claims::{self, Claims};
use super::detached;
use super::gates;
use super::jobs::{JobLeases, SpooledLeases};
use super::lane_probe;
use super::tickets;
use super::landings::{self, push_log};
use super::hops;
use super::host::Host;
use super::model::{Account, ProviderName, Delivery, LaunchRequest, SessionRecord, Status, Transcript};
use super::prompt::{PromptInputs, StandingOrders, render, variant_sha};
use super::records::{CLAIMS_RELEASED, PLACED, read_phased, read_record, write_record};
use super::model::Phases;
use super::remote::{self, HooksCheck};
use super::roster::Roster;
use super::runtime::{DEFAULT_FOREGROUND_CAP_SECS, Instructions, Opening, Runtime, now_ms};
use super::runtimes::Runtimes;
pub use super::watch::TurnWatch;
use super::unaccepted::Unaccepted;
use super::run_recording::{NoRecording, RunRecording};
use lanes::OverSshLanes;
pub use lanes::{RemoteLane, RemoteLanes};
use super::usage_sink::{NoUsage, UsageSink};
use super::sender::urgent_prefixed;
use super::store::{OverSsh, Registrar, SPOOL_DIR, Spool};
use super::waits::Waits;
use super::transcript::{current_session_stream_index, ensure_session_streams, read_window, session_stream_path, stream_len};

mod adopting;
mod awaiting;
use awaiting::{await_conversation, await_reply, await_settled};
mod claiming;
mod control;
mod depth;
pub mod efforts;
mod first_turn;
mod follower;
mod gate_commands;
mod gate_rows;
mod delivery;
mod hand_in;
mod handoff;
mod inheriting;
mod interactive;
mod kept;
mod lanes;
mod relaunching;
pub use relaunching::Restart;
mod reopening;
mod retiring;
mod launching;
mod home;
mod start_lock;
mod stopping;
mod withdrawing;
mod spend_rows;
mod takeover;
mod wait_rows;
mod table;
pub use home::{orders_here, recorded_here, retired_report_here, root_here};
pub use lanes::{RunningLane, worktree_of};
mod messages;
mod owners;
mod parents;
mod past;
mod senders;
mod status;
mod rebooting;
mod stranding;
mod proxy_window;
mod testing;
#[doc(hidden)]
pub mod fixtures;

pub use follower::Follower;
pub use home::roster;
use home::{fleet_root, home_dir, live_state_dir, state_dir};
pub use hand_in::HandedIn;
pub use control::Controlled;
pub use relaunching::{Choices, REVIVED_MID_TURN};
pub use stopping::Halt;
pub use gate_rows::Opened;
pub use takeover::backlog_line;
pub use messages::Sent;
pub use spend_rows::Spent;
pub use wait_rows::Waiting;
pub use proxy_window::{NoDesk, OpenedWindows, ProxyDesk};

const DEFAULT_IDLE_EXIT_MINUTES: u64 = 10;
const HOSTS: &str = ".hosts";

#[derive(Clone)]
pub struct Fleet {
    pub(crate) root: PathBuf,
    state: PathBuf,
    home: PathBuf,
    policy: PathBuf,
    runtimes: Runtimes,
    ledger: Arc<dyn FleetBooks>,
    claims: Arc<dyn Claims>,
    host: String,
    arm: fn(i64) -> NorthResult<()>,
    registrar: Arc<dyn Registrar>,
    leases: Arc<dyn JobLeases>,
    proc: PathBuf,
    cgroups: PathBuf,
    admission: PathBuf,
    usage: Arc<dyn UsageSink>,
    recording: Arc<dyn RunRecording>,
    remote_lanes: Arc<dyn RemoteLanes>,
    desk: Arc<dyn ProxyDesk>,
    spools_as: Option<String>,
}

struct Ports {
    runtimes: Runtimes,
    ledger: Arc<dyn FleetBooks>,
    claims: Arc<dyn Claims>,
    registrar: Arc<dyn Registrar>,
    leases: Arc<dyn JobLeases>,
}

impl Fleet {
    pub fn open(runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>, registrar: Arc<dyn Registrar>, leases: Arc<dyn JobLeases>) -> NorthResult<Self> {
        let home = home_dir()?;
        let state = live_state_dir(&home);
        north_base::platform::land_queue::confined(&state).map_err(NorthError::Refused)?;
        let fleet = Self::rooted_at(home, state, crate::platform::orchestration::path(), Ports { runtimes, ledger, claims, registrar, leases })?;
        Ok(Self { spools_as: north_base::platform::session_scope::spooling_worker(), ..fleet })
    }

    fn rooted(home: PathBuf, policy: PathBuf, runtimes: Runtimes, ledger: Arc<dyn FleetBooks>, claims: Arc<dyn Claims>, registrar: Arc<dyn Registrar>, leases: Arc<dyn JobLeases>) -> NorthResult<Self> {
        let state = state_dir(&home);
        Self::rooted_at(home, state, policy, Ports { runtimes, ledger, claims, registrar, leases })
    }

    fn rooted_at(home: PathBuf, state: PathBuf, policy: PathBuf, ports: Ports) -> NorthResult<Self> {
        let Ports { runtimes, ledger, claims, registrar, leases } = ports;
        let root = fleet_root(&state);
        fs::create_dir_all(&root)?;
        let kept = root.join(lanes::KEPT_LANES);
        let fleet = Self { root, state, home, policy, runtimes, ledger, claims, host: crate::platform::host::name(), arm: parents::await_default, registrar, leases, proc: "/proc".into(), cgroups: "/sys/fs/cgroup".into(), admission: crate::platform::admission_root::here(), usage: Arc::new(NoUsage), recording: Arc::new(NoRecording), remote_lanes: Arc::new(OverSshLanes { kept }), desk: Arc::new(NoDesk), spools_as: None };
        fleet.adopt_journals()?;
        Ok(fleet)
    }

    pub fn writing_usage(mut self, usage: Arc<dyn UsageSink>) -> Self {
        self.usage = usage;
        self
    }

    pub fn recording_runs(mut self, recording: Arc<dyn RunRecording>) -> Self {
        self.recording = recording;
        self
    }

    /// Shows the proxy on `desk`, the desktop Tom types on; a fleet without one starts no proxy.
    pub fn showing_proxy_on(mut self, desk: Arc<dyn ProxyDesk>) -> Self {
        self.desk = desk;
        self
    }

    pub fn watch(&self, provider: &ProviderName, host_dir: &Path) -> NorthResult<TurnWatch> {
        Ok(TurnWatch::new(self.runtimes.get(provider)?, self.ledger.clone(), Arc::new(self.clone()), self.claims.clone(), self.root.clone(), host_dir.to_path_buf()).reading_lanes(Arc::new(lane_probe::Git)).writing_usage(self.usage.clone()).recording_runs(self.recording.clone()).reading_tickets(Arc::new(tickets::LandQueue::here())).reading_jobs(Arc::new(SpooledLeases::new(self.leases.clone(), Spool::at(&self.root)).reading_far(Arc::new(OverSsh::default())))).reading_scope(Arc::new(detached::Procfs::here().after(detached::Prior::all(host_dir)))))
    }

    /// Every phase move of this host's sessions, in the lifecycle store beside the fleet root.
    pub fn phases(&self) -> NorthResult<Phases> {
        Phases::open(&self.root)
    }

    pub fn waits(&self) -> Waits {
        Waits::in_root(&self.root)
    }

    pub fn orders(&self) -> StandingOrders {
        StandingOrders::in_state(&self.state)
    }

    pub fn roster(&self) -> Roster {
        self.runtimes.roster(&state_dir(&self.home), &self.home, &self.policy)
    }

    pub fn placed_hosts(&self) -> Option<Vec<String>> {
        crate::platform::orchestration::placed_hosts(&self.policy)
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.roster().accounts
    }

    pub fn providers(&self) -> Vec<ProviderName> {
        self.runtimes.providers()
    }

    pub fn sessions(&self) -> NorthResult<Vec<SessionRecord>> {
        let mut phases = Phases::reading(&self.root);
        let mut sessions: Vec<SessionRecord> = fs::read_dir(&self.root)?
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|entry| read_phased(phases.as_mut(), &entry.path()).ok())
            .collect();
        sessions.sort_by_key(|record| record.created_ms);
        Ok(sessions)
    }

    pub fn session(&self, name: &str) -> NorthResult<SessionRecord> {
        self.record(name)
    }

    /// A session is running while a live host serves it, or, adopted, while its interactive window lives.
    pub fn running(&self, record: &SessionRecord) -> bool {
        if record.adopted() {
            return self.window_status(record) != Status::Exited;
        }
        session_running(record)
    }

    fn prompt(&self, name: &str, id: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        let held = self.record(name)?;
        if held.adopted() {
            return self.tell_window(&held, text);
        }
        let record = self.wake(name)?;
        match self.prompt_live(&record, id, text, delivery) {
            Err(_) if Host::new(record.host_dir.clone()).closed() => {
                Host::new(record.host_dir.clone()).await_end()?;
                self.prompt_live(&self.wake(name)?, id, text, delivery)
            }
            sent => sent,
        }
    }

    fn prompt_live(&self, record: &SessionRecord, id: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        let transcript = self.recent(self.runtime(record)?.as_ref(), record)?;
        match delivery {
            Delivery::Now if transcript.status != Status::Idle => self.send_urgent(record, &transcript, id, text),
            _ => self.send_text(record, &transcript, id, text),
        }
    }

    pub fn interrupt(&self, name: &str) -> NorthResult<()> {
        self.interrupt_for(name, Halt::Ordered)
    }

    /// Interrupts `name`'s turn; a turn it cuts short records `why` as its cause.
    pub(super) fn interrupt_for(&self, name: &str, why: Halt) -> NorthResult<()> {
        let record = self.record(name)?;
        if record.adopted() {
            return Err(NorthError::Usage(format!("{name} is an interactive window; interrupt it there")));
        }
        let runtime = self.runtime(&record)?;
        let transcript = self.recent(runtime.as_ref(), &record)?;
        Host::new(record.host_dir.clone()).send(&runtime.interrupt(&transcript)?)?;
        if transcript.status == Status::Working {
            why.record(name);
        }
        Ok(())
    }

    pub fn observe_landings(&self) -> NorthResult<()> {
        let mut log = landings::PushLog::read(&push_log());
        for record in self.sessions()? {
            landings::observe_among(self.ledger.as_ref(), &record, &log.of(&record))?;
        }
        Ok(())
    }

    pub fn spend_of(&self, name: &str) -> NorthResult<Option<SessionSpend>> {
        Ok(self.ledger.spend()?.into_iter().find(|spend| spend.session == name))
    }

    pub fn books_revision(&self) -> NorthResult<Option<Revision>> {
        self.ledger.revision()
    }

    pub fn digest(&self, since_ms: u64) -> NorthResult<Digest> {
        Ok(Digest { gates: gates::decisions(self.ledger.gates(since_ms)?), spend: self.ledger.spend()?, landed: self.ledger.landed_since(since_ms)? })
    }

    pub fn status(&self, record: &SessionRecord) -> Status {
        if record.adopted() {
            return self.window_status(record);
        }
        if record.host_dir.as_os_str().is_empty() {
            return status::settled(record, Status::Starting);
        }
        self.status_from_index(record).unwrap_or_else(|_| status::settled(record, Status::Starting))
    }

    pub fn follow(&self, name: &str) -> NorthResult<Follower> {
        self.follow_with_history(name, true)
    }

    /// A retired session's transcript read from its archive: the reaper moves a session out of the registry once its report is final, and dispatch reads that report there.
    pub fn follow_retired(&self, name: &str) -> NorthResult<Follower> {
        let (dir, mut record) = self.retired_session(name).ok_or_else(|| NorthError::Usage(format!("no session named {name}")))?;
        if record.host_dir == self.root.join(name) {
            record.host_dir = dir.clone();
        }
        let (path, indexed_to) = self.stream_index_among(&record, || Ok(vec![record.clone()]))?;
        Follower::open(self.runtime(&record)?, record, dir, path, indexed_to, true)
    }

    pub fn follow_recent(&self, name: &str) -> NorthResult<Follower> {
        self.follow_with_history(name, false)
    }

    fn follow_with_history(&self, name: &str, full_history: bool) -> NorthResult<Follower> {
        let record = self.record(name)?;
        if record.adopted() {
            let status = self.window_status(&record);
            return Ok(Follower::window(self.runtime(&record)?, record, self.root.join(name), status));
        }
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
        self.observed_status(record).map(|observed| status::settled(record, observed))
    }

    fn stream_index(&self, record: &SessionRecord) -> NorthResult<(PathBuf, u64)> {
        self.stream_index_among(record, || Ok(self.sessions()?.into_iter().filter(|session| session.host_dir == record.host_dir).collect()))
    }

    fn stream_index_among(&self, record: &SessionRecord, siblings: impl FnOnce() -> NorthResult<Vec<SessionRecord>>) -> NorthResult<(PathBuf, u64)> {
        if record.host_dir.as_os_str().is_empty() {
            return Err(NorthError::State(format!("{} has no host stream", record.name)));
        }
        if let Some(indexed_to) = current_session_stream_index(&record.host_dir, &record.name) {
            return Ok((session_stream_path(&record.host_dir, &record.name), indexed_to));
        }
        let records = siblings()?;
        let runtime = self.runtime(record)?;
        let indexed_to = ensure_session_streams(&record.host_dir, &records, |frame| runtime.frame_conversation(frame).map(str::to_owned))?;
        Ok((session_stream_path(&record.host_dir, &record.name), indexed_to))
    }

    fn start(&self, record: &mut SessionRecord, account: &Account, opening: Opening, hooks: HooksCheck) -> NorthResult<()> {
        let opened = self.open_host(record, account, opening)?;
        self.go_live(record, opening, hooks, opened)
    }

    /// Starts the session's host, and with it the provider CLI, or joins the shared host it runs in; the session is not yet recorded live, so a launch runs this beside its registration and sends nothing but the CLI's handshake until that is in.
    fn open_host(&self, record: &SessionRecord, account: &Account, opening: Opening) -> NorthResult<OpenedHost> {
        let since_ms = if record.host_dir.as_os_str().is_empty() { record.created_ms } else { now_ms() };
        self.refuse_on_window(record.conversation.as_deref())?;
        let runtime = self.runtime(record)?;
        self.refuse_headless_proxy(record, runtime.as_ref(), account)?;
        let (instructions, sha) = self.instructions(record)?;
        let trace = hops::trace_in(&self.root.join(&record.name));
        let spec = remote::placed(runtime.host(account, &record.relaunch_request(), opening, &instructions)?, record, &trace)?;
        let host_dir = if spec.shared { self.root.join(HOSTS).join(&spec.key) } else { self.root.join(&record.name) };
        let host = Host::new(host_dir.clone());
        let started = !host.running();
        if started {
            host.start(&spec, &record.provider, idle_exit(record), &account.recorded_label())?;
            host.send(&runtime.handshake())?;
        }
        Ok(OpenedHost { runtime, instructions, sha, trace, host_dir, host, started, since_ms })
    }

    fn go_live(&self, record: &mut SessionRecord, opening: Opening, hooks: HooksCheck, opened: OpenedHost) -> NorthResult<()> {
        let OpenedHost { runtime, instructions, sha, trace, host_dir, host, started, since_ms } = opened;
        let request = record.relaunch_request();
        let changed = record.system_prompt_sha.as_deref() != Some(sha.as_str());
        record.system_prompt_sha = Some(sha);
        if let Some(reason) = hooks.verdict()? {
            if started {
                host.terminate()?;
            }
            return Err(NorthError::Refused(reason));
        }
        if record.host_dir != host_dir {
            let _ = fs::remove_file(session_stream_path(&host_dir, &record.name));
            record.stream_offset = stream_len(&host_dir);
            record.host_dir = host_dir.clone();
        }
        record.host_pid = host.pid();
        write_record(&self.root.join(&record.name), record)?;
        let cause = if started { "a session host started for it" } else { "it joined its running session host" };
        let phase = self.phases()?.live_with(record.launch(), record.host_pid, cause, &Self::watchdog(record))?;
        hops::record(&trace, &hops::Span { session: record.name.clone(), hop: hops::Hop::SessionHost, start_ms: since_ms, end_ms: now_ms() });
        record.phase = Some(phase);
        match fs::remove_file(self.root.join(&record.name).join(CLAIMS_RELEASED)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        self.start_watchdogs()?;
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
        claims::hold_for(self.claims.as_ref(), record, &self.ancestors(record.parent.as_deref()));
        self.show_proxy(record, &host)?;
        Ok(())
    }

    fn instructions(&self, record: &SessionRecord) -> NorthResult<(String, String)> {
        let orders = self.orders().read()?;
        let extra = record.system_prompt.as_deref().map(fs::read_to_string).transpose()?;
        let inputs = PromptInputs {
            session: &record.name,
            role: record.role(),
            mode: record.prompt_mode,
            orders: &orders,
            foreground_cap_secs: record.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS),
            extra: extra.as_deref(),
        };
        Ok((render(&inputs), variant_sha(&inputs)))
    }

    fn send_text(&self, record: &SessionRecord, transcript: &Transcript, id: &str, text: &str) -> NorthResult<()> {
        Host::new(record.host_dir.clone()).send(&self.runtime(record)?.message(transcript, id, text)?)
    }

    fn send_urgent(&self, record: &SessionRecord, transcript: &Transcript, id: &str, text: &str) -> NorthResult<()> {
        let runtime = self.runtime(record)?;
        let interrupted_from = stream_len(&record.host_dir);
        Host::new(record.host_dir.clone()).send(&runtime.interrupt(transcript)?)?;
        let settled = await_settled(runtime.as_ref(), record, transcript.clone(), interrupted_from)?;
        self.send_text(record, &settled, id, &urgent_prefixed(text))
    }

    fn host_in_use(&self, stopping: &SessionRecord) -> NorthResult<bool> {
        Ok(self.sessions()?.iter().any(|other| {
            other.name != stopping.name && other.host_dir == stopping.host_dir && session_running(other)
        }))
    }

    pub(crate) fn account(&self, request: &LaunchRequest) -> NorthResult<Account> {
        self.runtimes.account(&state_dir(&self.home), &self.home, &self.policy, request)
    }

    fn runtime(&self, record: &SessionRecord) -> NorthResult<Arc<dyn Runtime>> {
        self.runtimes.get(&record.provider)
    }

    pub(crate) fn record(&self, name: &str) -> NorthResult<SessionRecord> {
        read_record(&self.session_dir(name)?).map_err(|_| NorthError::Usage(format!("no session named {name}")))
    }

    pub fn session_dir(&self, name: &str) -> NorthResult<PathBuf> {
        let valid = !name.is_empty() && !name.starts_with('.') && !name.contains('/') && name != SPOOL_DIR;
        if !valid {
            return Err(NorthError::Usage(format!("invalid session name {name:?}")));
        }
        Ok(self.root.join(name))
    }
}

/// A session host a start has running, or joined, and has not yet recorded live.
struct OpenedHost {
    runtime: Arc<dyn Runtime>,
    instructions: String,
    sha: String,
    trace: PathBuf,
    host_dir: PathBuf,
    host: Host,
    started: bool,
    since_ms: u64,
}

impl OpenedHost {
    fn abandon(self) -> NorthResult<()> {
        if self.started { self.host.terminate() } else { Ok(()) }
    }
}

fn idle_exit(record: &SessionRecord) -> Option<Duration> {
    let minutes = record.idle_exit_minutes.unwrap_or(DEFAULT_IDLE_EXIT_MINUTES);
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}

fn session_running(record: &SessionRecord) -> bool {
    record.live()
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
