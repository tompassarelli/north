//! A land ticket's phase as one `north_base::lifecycle` record on the lander's host: the lander and every command that
//! moves a ticket go through `Store::transition`, and the doctor, the fleet and the flow reports read the record
//! instead of the ticket's queue directory or the first word of its verdict (docs/rationale/state-machines.md, entry 2).
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;
use crate::lifecycle::{Lifecycle, Refusal, State, Store};

/// The longest a healthy land check holds a ticket: the land-check ceiling with room for a slow candidate build.
const CHECK_DWELL: Duration = Duration::from_secs(2 * 3600);

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
            Err(Refusal::Illegal { from, to }) => Err(failure(format!("{} was {}, so it cannot become {}", path.display(), from.map_or("new", State::name), to.name()))),
            Err(Refusal::Held { owner, .. }) => Err(failure(format!("{}: {owner} is checking it, so it moves once that check ends", path.display()))),
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
