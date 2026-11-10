//! The window's escalation ledger as `north dash` shows it: every launch a spin signal climbed to another rung,
//! grouped by the rung it landed on, and the lift check's landed rate for the higher rung against the parallel
//! lower rung. A lift try records the escalation it shadows in `lift`, so the pair is read from the dispatch
//! record rather than guessed from timing (north#940).
use std::collections::{BTreeMap, HashMap};

use north_dispatch::dispatch::{LaunchRecord, Rung};
use serde::Serialize;

use crate::ledger::WindowRun;

/// One rung's escalations and the lift checks that ran beside them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RungRow {
    pub rung: String,
    pub escalations: usize,
    /// The escalations whose run ended, and how many of those landed.
    pub finished: usize,
    pub landed: usize,
    pub lift_checks: usize,
    pub higher_landed: usize,
    pub lower_landed: usize,
}

impl RungRow {
    pub fn lift_delta(&self) -> Option<f64> {
        (self.lift_checks > 0).then(|| {
            (self.higher_landed as f64 - self.lower_landed as f64) / self.lift_checks as f64
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Escalations {
    pub rows: Vec<RungRow>,
    pub lift_checks: usize,
    pub lift_higher_landed: usize,
    pub lift_lower_landed: usize,
}

/// Whether the ledger recorded an end for the run, so it can count toward a landed rate.
fn finished(outcome: &str) -> bool {
    !matches!(outcome, "" | "running" | "idle" | "split")
}

fn order(rung: Rung) -> usize {
    Rung::LADDER
        .iter()
        .position(|on| *on == rung)
        .unwrap_or(Rung::LADDER.len())
}

impl Escalations {
    pub fn of(records: &[LaunchRecord], runs: &[WindowRun], since: f64, now: f64) -> Self {
        let outcome = |name: &str| {
            runs.iter()
                .find(|run| run.agent == name)
                .map(|run| (run.outcome.as_str(), finished(&run.outcome)))
        };
        let in_window =
            |record: &&LaunchRecord| record.launched && record.at >= since && record.at < now;
        let mut lower: HashMap<&str, &LaunchRecord> = HashMap::new();
        for record in records.iter().filter(in_window) {
            if let Some(higher) = record.lift.as_deref() {
                lower.insert(higher, record);
            }
        }
        let mut rows: BTreeMap<(usize, String), RungRow> = BTreeMap::new();
        for record in records.iter().filter(in_window).filter(|record| record.signal.is_some()) {
            let Some(rung) = Rung::recorded(record) else {
                continue;
            };
            let name = rung.name();
            let row = rows.entry((order(rung), name.clone())).or_insert_with(|| RungRow {
                rung: name,
                escalations: 0,
                finished: 0,
                landed: 0,
                lift_checks: 0,
                higher_landed: 0,
                lower_landed: 0,
            });
            row.escalations += 1;
            let higher = outcome(&record.name);
            if let Some((higher_outcome, true)) = higher {
                row.finished += 1;
                row.landed += usize::from(higher_outcome == "done");
            }
            let Some(lower) = lower.get(record.name.as_str()) else {
                continue;
            };
            let Some(lower_rung) = Rung::recorded(lower) else {
                continue;
            };
            if lower_rung.above() != Some(rung) {
                continue;
            }
            let Some((lower_outcome, true)) = outcome(&lower.name) else {
                continue;
            };
            let Some((higher_outcome, true)) = higher else {
                continue;
            };
            row.lift_checks += 1;
            row.higher_landed += usize::from(higher_outcome == "done");
            row.lower_landed += usize::from(lower_outcome == "done");
        }
        let rows: Vec<RungRow> = rows.into_values().collect();
        let lift_checks = rows.iter().map(|row| row.lift_checks).sum();
        let lift_higher_landed = rows.iter().map(|row| row.higher_landed).sum();
        let lift_lower_landed = rows.iter().map(|row| row.lower_landed).sum();
        Self {
            rows,
            lift_checks,
            lift_higher_landed,
            lift_lower_landed,
        }
    }

    pub fn summary(&self, window: &str) -> String {
        let escalations: usize = self.rows.iter().map(|row| row.escalations).sum();
        if escalations == 0 {
            return format!("no box climbed a rung in the last {window}");
        }
        let rungs = if self.rows.len() == 1 {
            "rung"
        } else {
            "rungs"
        };
        let lift = if self.lift_checks == 0 {
            "no lift check has both runs ended yet".to_owned()
        } else {
            format!("{} lift checks compared", self.lift_checks)
        };
        format!(
            "{escalations} escalations on {} {rungs}; {lift}",
            self.rows.len()
        )
    }
}
