//! A land ticket's phase as one `north_base::lifecycle` record on the lander's host: the lander and every command that
//! moves a ticket go through `Store::transition`, and the doctor, the fleet and the flow reports read the record
//! instead of the ticket's queue directory or the first word of its verdict (docs/rationale/state-machines.md, entry 2).
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;
use crate::lifecycle::{Lifecycle, Lost, Refusal, State, Store};

/// The longest a healthy land check holds a ticket: the land-check ceiling with room for a slow candidate build.
const CHECK_DWELL: Duration = Duration::from_secs(2 * 3600);

/// The longest `safe-push --to main --wait` waits for the lander's verdict; a fleet session's foreground maximum sits above it.
pub const HAND_IN_WAIT: Duration = Duration::from_secs(25 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Queued,
    Checking,
    Landed,
    Failed,
    Conflict,
    Replaced,
    Withdrawn,
    Quarantined,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Queued, Self::Checking, Self::Landed, Self::Failed, Self::Conflict, Self::Replaced, Self::Withdrawn, Self::Quarantined];

    fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Checking => "checking",
            Self::Landed => "landed",
            Self::Failed => "failed",
            Self::Conflict => "conflict",
            Self::Replaced => "replaced",
            Self::Withdrawn => "withdrawn",
            Self::Quarantined => "quarantined",
        }
    }
}

impl Phase {
    /// The phase a decision's verdict puts its ticket in, read from the verdict's first word: the one place a verdict's
    /// text becomes a phase.
    pub fn decided(verdict: &str) -> Self {
        match verdict.split_whitespace().next() {
            Some("landed") => Self::Landed,
            Some("conflict") => Self::Conflict,
            Some("replaced") => Self::Replaced,
            Some("withdrawn") => Self::Withdrawn,
            Some("quarantined") => Self::Quarantined,
            _ => Self::Failed,
        }
    }

    pub fn is_decided(self) -> bool {
        !matches!(self, Self::Queued | Self::Checking)
    }

    /// The queue subdirectory a ticket in this phase lives in, `None` for the queue itself.
    pub fn area(self) -> Option<&'static str> {
        match self {
            Self::Queued | Self::Checking => None,
            Self::Withdrawn => Some("withdrawn"),
            Self::Quarantined => Some("quarantine"),
            Self::Landed | Self::Failed | Self::Conflict | Self::Replaced => Some("done"),
        }
    }
}

pub struct LandTicket;

impl Lifecycle for LandTicket {
    const NAME: &'static str = "land-ticket";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        use Phase::{Checking, Conflict, Failed, Quarantined, Queued};
        match (from, to) {
            (None, Checking) => false,
            (None, _) | (Some(Queued), _) | (Some(Failed | Conflict | Quarantined), Queued) => true,
            (Some(Checking), to) => to != Checking,
            _ => false,
        }
    }

    fn dwell(state: Phase) -> Option<Duration> {
        (state == Phase::Checking).then_some(CHECK_DWELL)
    }

    fn orphan(state: Phase) -> Option<Phase> {
        (state == Phase::Checking).then_some(Phase::Queued)
    }
}

/// The ledger holding a queue's ticket records: the host's `ledger.db` for a queue in north's state layout
/// (`STATE/north/land-queue/KEY`), and one inside the queue for a scratch queue such as the nightly drill's.
pub fn ledger(queue: &Path) -> PathBuf {
    match queue.parent() {
        Some(queues) if queues.file_name() == Some(OsStr::new("land-queue")) => queues.parent().unwrap_or(queues).join("ledger.db"),
        _ => queue.join("ledger.db"),
    }
}

/// The directories a queue keeps its tickets in: the queue itself for waiting tickets, then each decided area.
pub fn directories(queue: &Path) -> [PathBuf; 4] {
    ["", "done", "withdrawn", "quarantine"].map(|area| queue.join(area))
}

pub fn open(queue: &Path) -> NorthResult<Store> {
    Store::open(&ledger(queue))
}

/// A ticket's instance name: its queue's key and its file name, so one lander host's queues never share a record.
pub fn instance(queue: &Path, name: &OsStr) -> String {
    format!("{}/{}", queue.file_name().unwrap_or_default().to_string_lossy(), name.to_string_lossy())
}

/// The ledger holding `queue`'s records when one exists, for a reader that must not create it.
pub fn recorded(queue: &Path) -> Option<Store> {
    let ledger = ledger(queue);
    if !ledger.is_file() {
        return None;
    }
    Store::open(&ledger).ok()
}

/// The phase of the ticket file at `path` in `queue` whose verdict is `verdict`: its record's, or for a ticket its
/// lander has not adopted yet, the phase adoption records.
pub fn read(store: Option<&Store>, queue: &Path, path: &Path, verdict: Option<&str>) -> Phase {
    store.and_then(|store| phase(store, queue, path)).unwrap_or_else(|| unrecorded(queue, path, verdict))
}

/// The phase a ticket file with no record is adopted in. A copy in withdrawn/ or quarantine/ wins over one in done/ or
/// the queue, since a withdrawal or a quarantine overrules a check, so a ticket a crashed move left in two areas is
/// adopted where that decision put it.
pub fn unrecorded(queue: &Path, path: &Path, verdict: Option<&str>) -> Phase {
    let Some(name) = path.file_name() else { return Phase::Queued };
    let overruled = [Phase::Withdrawn, Phase::Quarantined].into_iter().find(|phase| phase.area().is_some_and(|area| queue.join(area).join(name).is_file()));
    overruled.unwrap_or_else(|| match path.parent().and_then(Path::file_name).and_then(OsStr::to_str) {
        Some(area) if path.parent() != Some(queue) && Phase::Landed.area() == Some(area) => Phase::decided(verdict.unwrap_or_default()),
        _ => Phase::Queued,
    })
}

/// The recorded phase of the ticket at `path` in `queue`, `None` when it has no record or the ledger cannot be read.
pub fn phase(store: &Store, queue: &Path, path: &Path) -> Option<Phase> {
    let name = path.file_name()?;
    store.current::<LandTicket>(&instance(queue, name)).ok().flatten().map(|record| record.state)
}

/// Whether the ticket at `path` in `queue` was ever put back after a failed check, which the lander doctor does once.
pub fn requeued_after_failure(store: &Store, queue: &Path, path: &Path) -> bool {
    let Some(name) = path.file_name() else { return false };
    store.history(LandTicket::NAME, &instance(queue, name)).is_ok_and(|moves| moves.iter().any(|logged| !logged.refused && logged.from.as_deref() == Some(Phase::Failed.name()) && logged.to == Phase::Queued.name()))
}

fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn refused(path: &Path, refusal: Refusal<Phase>) -> io::Error {
    match refusal {
        Refusal::Illegal { from, to } => failure(format!("{} was {}, so it cannot become {}", path.display(), from.map_or("new", State::name), to.name())),
        Refusal::Held { owner, .. } => failure(format!("{}: {owner} is checking it, so it moves once that check ends", path.display())),
    }
}

fn name(path: &Path) -> io::Result<&OsStr> {
    path.file_name().ok_or_else(|| failure(format!("{} names no ticket", path.display())))
}

pub struct Records {
    store: Store,
    queue: PathBuf,
}

impl Records {
    pub fn open(queue: &Path) -> io::Result<Self> {
        Ok(Self { store: open(queue).map_err(failure)?, queue: queue.to_owned() })
    }

    /// The records of the queue the ticket file at `path` sits in, top level.
    pub fn of(path: &Path) -> io::Result<Self> {
        Self::open(path.parent().ok_or_else(|| failure(format!("{} has no queue", path.display())))?)
    }

    pub fn phase(&self, path: &Path) -> Option<Phase> {
        phase(&self.store, &self.queue, path)
    }

    /// The cause of the ticket's last move, which for a decided ticket is its verdict.
    pub fn cause(&self, path: &Path) -> io::Result<Option<String>> {
        Ok(self.store.current::<LandTicket>(&instance(&self.queue, name(path)?)).map_err(failure)?.map(|record| record.cause))
    }

    pub fn moved(&mut self, path: &Path, to: Phase, cause: &str) -> io::Result<()> {
        match self.store.transition::<LandTicket>(&instance(&self.queue, name(path)?), to, cause, Timestamp::now()).map_err(failure)? {
            Ok(_) => Ok(()),
            Err(refusal) => Err(refused(path, refusal)),
        }
    }

    /// [`Self::moved`] for a ticket whose record must still be `from` (`None` for no record): when another writer moved
    /// it first nothing moves, and `Err` names the phase that writer left.
    pub fn moved_from(&mut self, path: &Path, from: Option<Phase>, to: Phase, cause: &str) -> io::Result<Result<(), Option<Phase>>> {
        match self.store.transition_from::<LandTicket>(&instance(&self.queue, name(path)?), from, to, cause, Timestamp::now()).map_err(failure)? {
            Ok(_) => Ok(Ok(())),
            Err(Lost::AlreadyMoved { now }) => Ok(Err(now)),
            Err(Lost::Refused(refusal)) => Err(refused(path, refusal)),
        }
    }

    /// Records the ticket file at `path`, which has no record, in the phase its areas and verdict put it in.
    pub fn adopted(&mut self, path: &Path, verdict: Option<&str>) -> io::Result<Phase> {
        let phase = unrecorded(&self.queue, path, verdict);
        let area = path.parent().filter(|parent| *parent != self.queue).and_then(Path::file_name).map_or_else(|| "the queue".into(), OsStr::to_string_lossy);
        self.moved(path, phase, &format!("adopted from {area}: {}", verdict.unwrap_or("undecided")))?;
        Ok(phase)
    }

    /// Holds the tickets a land check takes for this lander.
    pub fn checking(&mut self, paths: &[PathBuf]) -> io::Result<()> {
        paths.iter().try_for_each(|path| self.moved(path, Phase::Checking, "a land check took it"))
    }

    /// Puts back every ticket of `paths` a check took and ended without deciding.
    pub fn unchecked(&mut self, paths: &[PathBuf], cause: &str) -> io::Result<()> {
        for path in paths.iter().filter(|path| self.phase(path) == Some(Phase::Checking)).collect::<Vec<_>>() {
            self.moved(path, Phase::Queued, cause)?;
        }
        Ok(())
    }

    /// At a lander's start, puts back every ticket of this queue a lander that is gone was checking.
    pub fn put_back(&mut self) -> io::Result<()> {
        let prefix = instance(&self.queue, OsStr::new(""));
        let held: Vec<String> = self.store.instances::<LandTicket>().map_err(failure)?.into_iter().filter(|record| record.state == Phase::Checking && record.instance.starts_with(&prefix)).map(|record| record.instance).collect();
        for instance in held {
            if let Err(refusal) = self.store.transition::<LandTicket>(&instance, Phase::Queued, "put back at a lander's start", Timestamp::now()).map_err(failure)? {
                println!("north land: {instance} stays checking: {refusal:?}");
            }
        }
        Ok(())
    }
}

/// Records a ticket just written into its queue as queued.
pub fn queued(path: &Path, cause: &str) -> io::Result<()> {
    Records::of(path)?.moved(path, Phase::Queued, cause)
}

/// One branch a fleet session queued for landing: its newest ticket in a queue, the phase the lander's record puts it in,
/// when it got there and the verdict that moved it. The scorer's pending bar and the fleet's resume pass read a session's
/// landing from this and never from the words of its report (north#791, docs/rationale/attribution.md).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lane {
    pub queue: PathBuf,
    pub branch: String,
    pub phase: Phase,
    pub since: Option<Timestamp>,
    pub cause: Option<String>,
    queued: u128,
}

impl Lane {
    pub fn undecided(&self) -> bool {
        !self.phase.is_decided()
    }

    pub fn landed(&self) -> bool {
        self.phase == Phase::Landed
    }

    /// When the ticket was queued, in Unix milliseconds.
    pub fn queued_ms(&self) -> u64 {
        u64::try_from(self.queued / 1000).unwrap_or(u64::MAX)
    }

    /// When the lander's record put the ticket in its phase, in Unix milliseconds.
    pub fn since_ms(&self) -> Option<u64> {
        u64::try_from(self.since?.unix_millis()).ok()
    }
}

/// What opens the message that tells a session the lander decided its lane, which the fleet finds a wake by.
pub const DECIDED_OPENER: &str = "[north: the lander has decided]";

/// The message that starts a turn in a session whose lanes the lander refused, one `BRANCH: VERDICT` line each.
pub fn refusal_message(refused: &[(&str, &str)]) -> String {
    let lines: Vec<String> = refused.iter().map(|(branch, verdict)| format!("{branch}: {verdict}")).collect();
    format!("{DECIDED_OPENER}\n{}\nContinue from the verdict: fix what the refusal names and finish the lane again.", lines.join("\n"))
}

/// The lanes of `session` in every queue under `queues` (`STATE/north/land-queue`): per queue and branch the newest ticket
/// in any area. A host that is not the lander's holds no queue, so its answer is none.
pub fn lanes_of(queues: &Path, session: &str) -> NorthResult<Vec<Lane>> {
    Ok(read_lanes(queues, Some(session))?.remove(session).unwrap_or_default())
}

/// The lanes of every session under `queues`, read once.
pub fn lanes_by_session(queues: &Path) -> NorthResult<std::collections::BTreeMap<String, Vec<Lane>>> {
    read_lanes(queues, None)
}

fn read_lanes(queues: &Path, wanted: Option<&str>) -> NorthResult<std::collections::BTreeMap<String, Vec<Lane>>> {
    let entries = match std::fs::read_dir(queues) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(error.into()),
    };
    let mut newest: std::collections::BTreeMap<(PathBuf, String, String), Lane> = std::collections::BTreeMap::new();
    for queue in entries {
        let queue = queue?.path();
        if !queue.is_dir() {
            continue;
        }
        let store = recorded(&queue);
        for directory in directories(&queue) {
            for (path, queued, ticket) in tickets_in(&directory)? {
                let Some(session) = ticket["session"].as_str().filter(|session| wanted.is_none_or(|wanted| wanted == *session)) else { continue };
                let branch = ticket["branch"].as_str().unwrap_or_default().to_owned();
                let record = store.as_ref().zip(path.file_name()).and_then(|(store, name)| store.current::<LandTicket>(&instance(&queue, name)).ok().flatten());
                let phase = record.as_ref().map_or_else(|| unrecorded(&queue, &path, ticket["verdict"].as_str()), |record| record.state);
                let lane = Lane { queue: queue.clone(), branch: branch.clone(), phase, since: record.as_ref().map(|record| record.since), cause: record.map(|record| record.cause), queued };
                let slot = newest.entry((queue.clone(), session.to_owned(), branch)).or_insert_with(|| lane.clone());
                if queued >= slot.queued {
                    *slot = lane;
                }
            }
        }
    }
    let mut by_session: std::collections::BTreeMap<String, Vec<Lane>> = std::collections::BTreeMap::new();
    for ((_, session, _), lane) in newest {
        by_session.entry(session).or_default().push(lane);
    }
    Ok(by_session)
}

fn tickets_in(directory: &Path) -> NorthResult<Vec<(PathBuf, u128, serde_json::Value)>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut tickets = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !path.is_file() || path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let ticket: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        let named = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('-').next()).and_then(|micros| micros.parse().ok());
        let queued = ticket["queued_micros"].as_u64().map(u128::from).or(named).unwrap_or(u128::MAX);
        tickets.push((path, queued, ticket));
    }
    Ok(tickets)
}
