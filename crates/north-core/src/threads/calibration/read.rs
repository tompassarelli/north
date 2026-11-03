//! Worker runs and calibration rows as the ledger holds them.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use rusqlite::{Connection, Row as SqlRow};

use crate::router::pricing::PriceTable;
use crate::platform::run_roles::worker_where;

use super::boxes::{BoxRun, item_chains};
use super::{GROUPS, Run, cell_value, tier_key};

/// A runs-table row with the columns the calibration core reads.
#[derive(Clone, Debug)]
pub struct Raw {
    pub agent: String,
    pub follows: Option<String>,
    pub item: Option<String>,
    pub outcome: String,
    pub expects_landing: bool,
    pub landed: Option<i64>,
    pub tier: String,
    pub cells: [String; 5],
    pub minutes: Option<f64>,
    pub active_min: Option<f64>,
    pub wait_min: Option<f64>,
    pub eta_min: Option<f64>,
    pub cost_usd: Option<f64>,
    pub api_usd: Option<f64>,
    pub started: Option<String>,
    pub ended: String,
}

const COLUMNS: &str = "agent, follows, item, outcome, expects_landing, landed, tier, category, spec, scope, surface, verify, minutes, eta_min, cost_usd, started, ended, model, COALESCE(input_tokens, 0), COALESCE(cache_read_tokens, 0), COALESCE(tokens, 0), active_min, wait_min";

static PRICES: LazyLock<PriceTable> = LazyLock::new(|| PriceTable::load().expect("router/prices.json parses"));

impl Raw {
    fn read(row: &SqlRow) -> rusqlite::Result<Self> {
        let tier: String = row.get(6)?;
        let (input, cache_read, output): (i64, i64, i64) = (row.get(18)?, row.get(19)?, row.get(20)?);
        let model: Option<String> = row.get(17)?;
        let api_usd = (input + output > 0).then(|| PRICES.run_usd(model.as_deref(), &tier, input, cache_read, output));
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
            tier,
            cells: [cell(0)?, cell(1)?, cell(2)?, cell(3)?, cell(4)?],
            minutes: row.get(12)?,
            active_min: row.get(21)?,
            wait_min: row.get(22)?,
            eta_min: row.get(13)?,
            cost_usd: row.get(14)?,
            api_usd,
            started: row.get(15)?,
            ended: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
        })
    }

    pub fn unjudged(&self) -> bool {
        matches!(self.outcome.as_str(), "running" | "idle") || (self.outcome == "unclear" && self.item.as_deref().is_none_or(str::is_empty))
    }

    /// Reported Done, not followed by a later run, and landed when it was expected to land.
    pub fn succeeded(&self, followed: bool) -> bool {
        self.outcome == "done" && !followed && !(self.expects_landing && self.landed == Some(0))
    }

    /// The core's view of this run, its tier as a tier key.
    pub fn recommend_run(&self, ok: bool) -> Run {
        Run { cells: self.cells.clone(), tier: tier_key(&self.tier), minutes: self.minutes, active_min: self.active_min, wait_min: self.wait_min, cost_usd: self.cost_usd, api_usd: self.api_usd, outcome: self.outcome.clone(), ok, ..Run::default() }
    }
}

/// Worker runs matching `condition`, in table order.
pub fn raw_runs(db: &Connection, condition: &str, params: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<Raw>> {
    let mut statement = db.prepare(&format!("SELECT {COLUMNS} FROM runs WHERE {} AND {condition}", worker_where()))?;
    statement.query_map(params, Raw::read)?.collect()
}

/// A Done run counts as kept only when no other run on its item was staffed within this long of its end.
pub const RERUN_WINDOW_MIN: f64 = 24.0 * 60.0;

/// How an item's worker runs chain: each run's attempt number, and for each run that a later run on the item
/// re-ran within [`RERUN_WINDOW_MIN`], when that re-run ended.
pub struct Attempts {
    pub numbers: BTreeMap<String, usize>,
    pub reruns: BTreeMap<String, String>,
}

pub fn attempts(db: &Connection) -> rusqlite::Result<Attempts> {
    let rows = box_rows(db)?;
    let mut found = Attempts { numbers: BTreeMap::new(), reruns: BTreeMap::new() };
    for chain in item_chains(&rows, RERUN_WINDOW_MIN) {
        for (index, (_, run)) in chain.iter().enumerate() {
            found.numbers.insert(run.agent.clone(), index + 1);
            if let Some((_, next)) = chain.get(index + 1) {
                found.reruns.insert(run.agent.clone(), next.ended.clone());
            }
        }
    }
    Ok(found)
}

/// The agents some later worker run follows, by its Follows line or by re-running the same item within a day.
pub fn escalated(db: &Connection) -> rusqlite::Result<BTreeSet<String>> {
    let mut followed: BTreeSet<String> = db.prepare(&format!("SELECT follows FROM runs WHERE {} AND follows IS NOT NULL", worker_where()))?.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
    followed.extend(attempts(db)?.reruns.into_keys());
    Ok(followed)
}

/// Every settled worker run with its success and whether it was a first assignment, for walk-forward calibration.
pub fn calibration_runs(db: &Connection) -> rusqlite::Result<Vec<Run>> {
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
            active_min: raw.active_min,
            wait_min: raw.wait_min,
            eta_min: raw.eta_min,
            cost_usd: raw.cost_usd,
            api_usd: raw.api_usd,
            outcome: raw.outcome,
            ended: raw.ended,
        })
        .collect())
}

/// Finished worker runs that name an item, for box chains.
pub fn box_rows(db: &Connection) -> rusqlite::Result<Vec<BoxRun>> {
    let mut statement = db.prepare(&format!("SELECT agent, item, category, outcome, staffed, started, ended, landed, landed_at, cost_usd FROM runs WHERE {} AND outcome != 'running' AND item IS NOT NULL", worker_where()))?;
    statement
        .query_map([], |row| {
            Ok(BoxRun {
                agent: row.get(0)?,
                item: row.get(1)?,
                category: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                outcome: row.get(3)?,
                staffed: row.get(4)?,
                started: row.get(5)?,
                ended: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                landed: row.get::<_, Option<i64>>(7)?.is_some_and(|landed| landed != 0),
                landed_at: row.get(8)?,
                cost_usd: row.get(9)?,
            })
        })?
        .collect()
}
