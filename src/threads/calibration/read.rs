//! Worker runs and calibration rows as the ledger holds them.
use std::collections::BTreeSet;

use rusqlite::{Connection, Row as SqlRow};

use super::boxes::BoxRun;
use super::{GROUPS, Run, cell_value, tier_key};

/// A runs-table row with the columns the calibration core reads.
#[derive(Clone, Debug)]
pub(crate) struct Raw {
    pub agent: String,
    pub follows: Option<String>,
    pub item: Option<String>,
    pub outcome: String,
    pub expects_landing: bool,
    pub landed: Option<i64>,
    pub tier: String,
    pub cells: [String; 5],
    pub minutes: Option<f64>,
    pub eta_min: Option<f64>,
    pub cost_usd: Option<f64>,
    pub started: Option<String>,
    pub ended: String,
}

const COLUMNS: &str = "agent, follows, item, outcome, expects_landing, landed, tier, category, spec, scope, surface, verify, minutes, eta_min, cost_usd, started, ended";

impl Raw {
    fn read(row: &SqlRow) -> rusqlite::Result<Self> {
        let cell = |index: usize| -> rusqlite::Result<String> {
            let value: Option<String> = row.get(7 + index)?;
            Ok(cell_value(GROUPS[index], value.as_deref()))
        };
        Ok(Self {
            agent: row.get(0)?,
            follows: row.get(1)?,
            item: row.get(2)?,
            outcome: row.get(3)?,
            expects_landing: row.get::<_, Option<i64>>(4)?.is_some_and(|expects| expects != 0),
            landed: row.get(5)?,
            tier: row.get(6)?,
            cells: [cell(0)?, cell(1)?, cell(2)?, cell(3)?, cell(4)?],
            minutes: row.get(12)?,
            eta_min: row.get(13)?,
            cost_usd: row.get(14)?,
            started: row.get(15)?,
            ended: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
        })
    }

    pub(crate) fn unjudged(&self) -> bool {
        matches!(self.outcome.as_str(), "running" | "idle") || (self.outcome == "unclear" && self.item.as_deref().is_none_or(str::is_empty))
    }

    /// Reported Done, not followed by a later run, and landed when it was expected to land.
    pub(crate) fn succeeded(&self, followed: bool) -> bool {
        self.outcome == "done" && !followed && !(self.expects_landing && self.landed == Some(0))
    }

    /// The core's view of this run, its tier as a tier key.
    pub(crate) fn recommend_run(&self, ok: bool) -> Run {
        Run { cells: self.cells.clone(), tier: tier_key(&self.tier), minutes: self.minutes, cost_usd: self.cost_usd, outcome: self.outcome.clone(), ok, ..Run::default() }
    }
}

/// Worker runs matching `condition`, in table order.
pub(crate) fn raw_runs(db: &Connection, condition: &str, params: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<Raw>> {
    let mut statement = db.prepare(&format!("SELECT {COLUMNS} FROM runs WHERE role = 'worker' AND {condition}"))?;
    statement.query_map(params, Raw::read)?.collect()
}

/// The agents some later worker run follows.
pub(crate) fn escalated(db: &Connection) -> rusqlite::Result<BTreeSet<String>> {
    db.prepare("SELECT follows FROM runs WHERE role = 'worker' AND follows IS NOT NULL")?.query_map([], |row| row.get(0))?.collect()
}

/// Every settled worker run with its success and whether it was a first assignment, for walk-forward calibration.
pub(crate) fn calibration_runs(db: &Connection) -> rusqlite::Result<Vec<Run>> {
    let escalated = escalated(db)?;
    Ok(raw_runs(db, "outcome NOT IN ('split','running') AND ended IS NOT NULL", &[])?
        .into_iter()
        .map(|raw| Run {
            ok: raw.succeeded(escalated.contains(&raw.agent)),
            unjudged: raw.unjudged(),
            first: raw.follows.is_none(),
            cells: raw.cells,
            tier: raw.tier,
            minutes: raw.minutes,
            eta_min: raw.eta_min,
            cost_usd: raw.cost_usd,
            outcome: raw.outcome,
            ended: raw.ended,
        })
        .collect())
}

/// Finished worker runs that name an item, for box chains.
pub(crate) fn box_rows(db: &Connection) -> rusqlite::Result<Vec<BoxRun>> {
    let mut statement = db.prepare("SELECT item, category, outcome, staffed, started, ended, landed, landed_at FROM runs WHERE role = 'worker' AND outcome != 'running' AND item IS NOT NULL")?;
    statement
        .query_map([], |row| {
            Ok(BoxRun {
                item: row.get(0)?,
                category: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                outcome: row.get(2)?,
                staffed: row.get(3)?,
                started: row.get(4)?,
                ended: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                landed: row.get::<_, Option<i64>>(6)?.is_some_and(|landed| landed != 0),
                landed_at: row.get(7)?,
            })
        })?
        .collect()
}
