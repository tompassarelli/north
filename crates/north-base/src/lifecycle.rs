//! Explicit state machines: each machine names one typed state per instance and its legal moves, one record per
//! instance lives in a [`Store`], [`Store::transition`] is the only writer and refuses an illegal move, every move is
//! logged with its cause, and [`Store::faults`] finds an instance that overstayed its state or whose owner died
//! without knowing any machine's type, so `north health` judges every machine alike (docs/rationale/state-machines.md).
use std::fmt::Debug;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;

mod owner;
mod store;
#[cfg(test)]
mod tests;

pub use owner::{Liveness, Owner, Proc};
pub use store::Store;

/// One state of a machine, stored by its name.
pub trait State: Copy + Eq + Debug + 'static {
    const ALL: &'static [Self];

    fn name(self) -> &'static str;

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|state| state.name() == name)
    }
}

/// A state machine: its name in the store, its states and the moves between them.
pub trait Lifecycle {
    const NAME: &'static str;
    type State: State;

    /// Whether an instance in `from` (`None` before its first record) may move to `to`.
    fn legal(from: Option<Self::State>, to: Self::State) -> bool;

    /// The longest a healthy instance stays in `state`; `None` for a state it may rest in.
    fn dwell(state: Self::State) -> Option<Duration>;

    /// Where an instance goes when the process holding `state` dies; `None` for a state no process holds.
    fn orphan(state: Self::State) -> Option<Self::State>;

    /// How long an instance that came to rest in `state` keeps its record; `None` keeps it until it moves again.
    fn keep(state: Self::State) -> Option<Duration> {
        let _ = state;
        None
    }
}

/// The one record of an instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Record<S> {
    pub instance: String,
    pub state: S,
    pub since: Timestamp,
    /// When the state's dwell runs out, `None` for a resting state.
    pub due: Option<Timestamp>,
    /// The process holding the state, for a state a process holds.
    pub owner: Option<Owner>,
    pub cause: String,
    /// How many moves the instance has made.
    pub seq: i64,
}

/// A move the store refused; the record is unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal<S> {
    /// The machine has no move from `from` to `to`; logged, and `north health` reports it.
    Illegal { from: Option<S>, to: S },
    /// A live process holds the instance's state; not logged, since two takers racing is expected.
    Held { state: S, owner: Owner },
}

/// The record after an accepted move, or why the move was refused.
pub type Moved<S> = Result<Record<S>, Refusal<S>>;

/// One logged move.
#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub from: Option<String>,
    pub to: String,
    pub at: Timestamp,
    pub by: Owner,
    pub cause: String,
    pub refused: bool,
}

/// Why an instance is stuck or a writer misbehaved, judged from the store alone.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Fault {
    pub machine: String,
    pub instance: String,
    pub state: String,
    pub since_ms: i64,
    pub kind: FaultKind,
    pub cause: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum FaultKind {
    /// The state outlived its dwell.
    Overdue { due_ms: i64 },
    /// The process holding the state is gone.
    Orphaned { owner: Owner },
    /// A writer attempted a move the machine does not have.
    Illegal { to: String, at_ms: i64 },
}

impl Fault {
    pub fn line(&self, now: Timestamp) -> String {
        let minutes = |ms: i64| (now.unix_millis() - ms).max(0) / 60_000;
        let at = format!("{}/{} {}", self.machine, self.instance, self.state);
        match &self.kind {
            FaultKind::Overdue { due_ms } => format!("{at} for {} min, {} min past its limit: {}", minutes(self.since_ms), minutes(*due_ms), self.cause),
            FaultKind::Orphaned { owner } => format!("{at} for {} min, held by {owner}, which is gone: {}", minutes(self.since_ms), self.cause),
            FaultKind::Illegal { to, at_ms } => format!("{at}: a writer tried to move it to {to} {} min ago: {}", minutes(*at_ms), self.cause),
        }
    }
}
