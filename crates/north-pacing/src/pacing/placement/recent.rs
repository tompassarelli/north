//! Launches placed in the last seconds, which no host reading shows yet: the saturation reading is cached for up to a
//! minute and the one-minute load average lags a new session by about another. Placement charges each such launch a
//! session's expected load against the host it went to, and a launch reads those placements, decides and records its
//! own under one lock, so a burst of concurrent launches spreads instead of reading one unmoved reading (north#1191).
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::Path;

use north_strategy_types::placement::{Case, Decision, Outcome};
use north_strategy_types::{Envelope, Kind};
use serde::de::IgnoredAny;

use crate::capacity::admission::default_mib;
use crate::platform::decision_log;

const WINDOW: &str = "placement_reserve_seconds";
const SESSION_CORES: &str = "placement_session_cores";

/// This launch's turn at placement, held from reading the recent placements until its own decision is recorded.
pub struct Turn {
    _held: File,
}

impl Turn {
    /// Waits for the turn; None when the decision log is off or its lock cannot be opened, and the launch then decides
    /// on what it read.
    pub fn take() -> Option<Self> {
        let directory = decision_log::root()?.join(Kind::Placement.name());
        std::fs::create_dir_all(&directory).ok()?;
        let held = OpenOptions::new().create(true).truncate(false).write(true).open(directory.join("turn.lock")).ok()?;
        held.lock().ok()?;
        Some(Self { _held: held })
    }
}

/// Sets each machine's `placed_recently` from the placements the decision log holds inside the window before the case.
pub fn charge(case: &mut Case) {
    let Some(root) = decision_log::root() else { return };
    let placed = placed_since(&root, case.at - default_mib(WINDOW));
    for machine in &mut case.machines {
        machine.placed_recently = placed.get(&machine.name).copied().unwrap_or(0);
    }
}

fn placed_since(root: &Path, since: f64) -> BTreeMap<String, i64> {
    let mut placed = BTreeMap::new();
    for (_, line) in decision_log::lines(root, Kind::Placement, "jsonl", since, decision_log::now()) {
        let Ok(logged) = serde_json::from_str::<Envelope<IgnoredAny, Decision>>(&line) else { continue };
        if let (true, Outcome::Placed { machine: Some(machine), .. }) = (logged.at >= since, logged.decision.outcome) {
            *placed.entry(machine).or_insert(0) += 1;
        }
    }
    placed
}

impl super::gate::Machine {
    /// Demand per thread for a new session (docs/rationale/placement.md): the larger of the reading's one-minute load
    /// and its live sessions' share, plus a session's share for each launch placed there inside the window, which the
    /// reading does not show yet, over its threads; None without a backlog reading.
    pub fn projected_load(&self) -> Option<f64> {
        let backlog = self.saturation?.backlog?;
        let share = default_mib(SESSION_CORES);
        let resident = self.live.unwrap_or(0) as f64 * share;
        let charged = self.placed_recently as f64 * share;
        Some((backlog.load.max(resident) + charged) / f64::from(backlog.threads.max(1)))
    }

    pub(in crate::pacing) fn load_note(&self) -> String {
        let Some(load) = self.projected_load() else { return String::new() };
        match self.placed_recently {
            0 => format!(", load {load:.2} per thread"),
            placed => format!(", load {load:.2} per thread counting {placed} placed in the last {:.0} s", default_mib(WINDOW)),
        }
    }
}

#[cfg(test)]
#[path = "recent_tests.rs"]
mod tests;
