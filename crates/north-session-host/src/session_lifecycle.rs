//! A fleet session's lifecycle: one record per launch in this host's lifecycle store, keyed `NAME@CREATED_MS`, so
//! whether a session is live, parked, ended or retired is never again spread over session.json flags and a host pid
//! two readers judged differently (docs/rationale/state-machines.md, north#757 entry 1).
use std::path::{Path, PathBuf};
use std::time::Duration;

use north_base::lifecycle::{Effect, Leased, Lifecycle, Owner, Refusal, State, Store};
use serde_json::Value;

use north_base::error::{NorthError, NorthResult};
use north_base::foundation::clock::Timestamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// A session host serves it; held by that host's process, so the host's death parks it.
    Live,
    /// No host serves it; a message revives it.
    Parked,
    /// Its host ended it at idle after its final report.
    IdleExited,
    /// Its host ended after its final report.
    HostEnded,
    /// Stopped by a command: stop, forget, relaunch, an interactive hand-off or a take-over.
    Stopped,
    /// Bound to an interactive window, which north does not host.
    Adopted,
    /// Archived; the end of the record.
    Retired,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Live, Self::Parked, Self::IdleExited, Self::HostEnded, Self::Stopped, Self::Adopted, Self::Retired];

    fn name(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Parked => "parked",
            Self::IdleExited => "idle-exited",
            Self::HostEnded => "host-ended",
            Self::Stopped => "stopped",
            Self::Adopted => "adopted",
            Self::Retired => "retired",
        }
    }
}

impl Phase {
    /// The session ended: nothing revives it unasked, and `fleet list` shows it exited.
    pub fn ended(self) -> bool {
        matches!(self, Self::IdleExited | Self::HostEnded | Self::Stopped | Self::Retired)
    }

    /// The label `fleet list --json` has always printed as `ended`.
    pub fn ending(self) -> Option<&'static str> {
        match self {
            Self::IdleExited => Some("idle-exit"),
            Self::HostEnded => Some("host-ended"),
            Self::Stopped => Some("stopped"),
            Self::Retired => Some("retired"),
            Self::Live | Self::Parked | Self::Adopted => None,
        }
    }
}

pub struct Session;

impl Lifecycle for Session {
    const NAME: &'static str = "fleet-session";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        use Phase::{Adopted, HostEnded, IdleExited, Live, Parked, Retired, Stopped};
        match (from, to) {
            (None, _) => true,
            (Some(Retired), _) | (Some(Adopted), Live) => false,
            (Some(_), Live) | (Some(Live), Parked | IdleExited | HostEnded) | (Some(_), Stopped) | (Some(Stopped), Retired) | (Some(Adopted), Adopted) => true,
            _ => false,
        }
    }

    fn dwell(_: Phase) -> Option<Duration> {
        None
    }

    fn orphan(state: Phase) -> Option<Phase> {
        (state == Phase::Live).then_some(Phase::Parked)
    }

    fn preempts(from: Phase, to: Phase) -> bool {
        from == Phase::Live && matches!(to, Phase::Stopped | Phase::Live)
    }
}

/// Claims `path` for one launch: `create_dir` lets only one of two racing launches win, so a name has one launch at a
/// time (north#1042).
pub fn claim(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(path)
}

/// The launch a session record names: one lifecycle instance per launch, so a name a later launch reuses starts
/// afresh.
#[derive(Clone, Copy, Debug)]
pub struct Launch<'a> {
    pub name: &'a str,
    pub created_ms: u64,
    /// A window binding rather than a hosted session.
    pub adopted: bool,
    pub host_pid: Option<u32>,
}

impl Launch<'_> {
    pub fn instance(&self) -> String {
        format!("{}@{}", self.name, self.created_ms)
    }
}

/// The lifecycle store of the fleet rooted at `root`: `STATE/ledger.db`, where `north health` reads its faults, for
/// a host's fleet at `STATE/fleet`; a fleet rooted anywhere else keeps its own.
/// The directory under a fleet root that holds archived sessions.
pub const ARCHIVE: &str = ".archive";

pub fn store_path(root: &Path) -> PathBuf {
    match root.parent() {
        Some(state) if root.file_name().is_some_and(|name| name == "fleet") => state.join("ledger.db"),
        _ => root.join(".ledger.db"),
    }
}

/// Every phase move of fleet sessions on this host.
pub struct Phases {
    store: Store,
}

impl Phases {
    pub fn open(root: &Path) -> NorthResult<Self> {
        Ok(Self { store: Store::open(&store_path(root))? })
    }

    pub fn open_as(root: &Path, who: Owner, liveness: Box<dyn north_base::lifecycle::Liveness>) -> NorthResult<Self> {
        Ok(Self { store: Store::open_as(&store_path(root), who, liveness)? })
    }

    /// The store of the fleet a session directory belongs to, an archived one (`ROOT/.archive/NAME-MS`) included, or
    /// `None`, said on stderr, when it cannot be opened.
    pub fn beside(session_dir: &Path) -> Option<Self> {
        let parent = session_dir.parent().unwrap_or(session_dir);
        let archived = parent.file_name().is_some_and(|name| name == ARCHIVE);
        Self::reading(if archived { parent.parent().unwrap_or(parent) } else { parent })
    }

    /// The store beside `root`, or `None`, said on stderr, when it cannot be opened.
    pub fn reading(root: &Path) -> Option<Self> {
        Self::open(root).map_err(|error| eprintln!("north fleet: the session lifecycle store beside {} is unreadable: {}", root.display(), error.user_message())).ok()
    }

    /// The session's phase, or `None`, said on stderr, when the store cannot give one.
    pub fn read(&mut self, record: Launch<'_>, legacy: &Value) -> Option<Phase> {
        self.of(record, legacy).map_err(|error| eprintln!("north fleet: {} has no phase: {}", record.name, error.user_message())).ok()
    }

    /// The session's phase, its dead host's Live first moved to Parked; a launch with no record is adopted once from
    /// the `stopped` and `ended` fields session.json kept before this machine.
    pub fn of(&mut self, record: Launch<'_>, legacy: &Value) -> NorthResult<Phase> {
        let now = Timestamp::now();
        if let Some(settled) = self.store.settle::<Session>(&record.instance(), now)? {
            return Ok(settled.state);
        }
        let (phase, holder) = classify(record, legacy);
        let cause = "adopted from session.json";
        let adopted = match holder {
            Some(holder) => self.store.transition_for::<Session>(&record.instance(), phase, cause, now, &holder)?,
            None => self.store.transition::<Session>(&record.instance(), phase, cause, now)?,
        };
        Ok(adopted.map_or(phase, |adopted| adopted.state))
    }

    /// Records that a host now serves the session, held by that host's process; a host gone before it could be
    /// read leaves the session Parked.
    pub fn live(&mut self, record: Launch<'_>, host_pid: Option<u32>, cause: &str) -> NorthResult<Phase> {
        self.live_with(record, host_pid, cause, &[])
    }

    /// [`Self::live`] that commits `effects` with the move to Live; a host gone before it could be read queues none.
    pub fn live_with(&mut self, record: Launch<'_>, host_pid: Option<u32>, cause: &str, effects: &[Effect]) -> NorthResult<Phase> {
        let now = Timestamp::now();
        let moved = match host_pid.and_then(Owner::of) {
            Some(host) => self.store.transition_with_effects::<Session>(&record.instance(), Phase::Live, cause, now, Some(&host), effects)?,
            None => self.store.transition::<Session>(&record.instance(), Phase::Parked, &format!("{cause}; its host was gone before it could be read"), now)?,
        };
        admitted(record, moved.map(|moved| moved.state))
    }

    pub fn moved(&mut self, record: Launch<'_>, to: Phase, cause: &str) -> NorthResult<Phase> {
        let moved = self.store.transition::<Session>(&record.instance(), to, cause, Timestamp::now())?;
        admitted(record, moved.map(|moved| moved.state))
    }

    pub fn current(&self, record: Launch<'_>) -> NorthResult<Option<Phase>> {
        Ok(self.store.current::<Session>(&record.instance())?.map(|current| current.state))
    }

    /// Sends every effect of `kind` this process may deliver, as [`Store::deliver`] does; returns how many were delivered.
    pub fn deliver(&mut self, kind: &str, hold: Duration, send: impl FnMut(&Leased) -> Result<(), String>) -> NorthResult<usize> {
        self.store.deliver(kind, Timestamp::now(), hold, send)
    }
}

/// The phase session.json's old fields and the host's process imply, and the host holding a live session.
pub fn classify(record: Launch<'_>, legacy: &Value) -> (Phase, Option<Owner>) {
    let stopped = legacy.get("stopped").and_then(Value::as_bool).unwrap_or(false);
    let ending = legacy.get("ended").and_then(Value::as_str);
    if record.adopted {
        return (if stopped { Phase::Stopped } else { Phase::Adopted }, None);
    }
    match (stopped, ending) {
        (true, Some("retired")) => (Phase::Retired, None),
        (true, Some("idle-exit")) => (Phase::IdleExited, None),
        (true, Some("host-ended")) => (Phase::HostEnded, None),
        (true, _) => (Phase::Stopped, None),
        (false, _) => match record.host_pid.and_then(Owner::of) {
            Some(host) => (Phase::Live, Some(host)),
            None => (Phase::Parked, None),
        },
    }
}

fn admitted(record: Launch<'_>, moved: Result<Phase, Refusal<Phase>>) -> NorthResult<Phase> {
    moved.map_err(|refusal| match refusal {
        Refusal::Illegal { from, to } => NorthError::Refused(format!("{}: a session cannot move from {} to {}", record.name, from.map_or("nothing", Phase::name), to.name())),
        Refusal::Held { state, owner } => NorthError::Refused(format!("{}: {} is held by {owner}", record.name, state.name())),
    })
}
