//! Tier success as a prediction: the shrunk success probability dispatch and `threads recommend` give an Item's
//! first run on a tier, and its walk-forward Brier score per category against the category's base rate.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};

use super::read::{escalated, raw_runs};
use super::tier_key;
use crate::foundation::clock::Timestamp;

pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS tier_predictions(item TEXT NOT NULL, tier TEXT NOT NULL, category TEXT NOT NULL, p REAL NOT NULL CHECK(p >= 0 AND p <= 1), at TEXT NOT NULL, source TEXT NOT NULL CHECK(source IN ('dispatch','recommend')), PRIMARY KEY(item, tier, at, source)) STRICT";

/// Who gave a prediction: dispatch for the tier it launched, recommend for each tier it listed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Dispatch,
    Recommend,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Recommend => "recommend",
        }
    }
}

/// Stores each tier's success probability for the Item's first run.
pub fn record(db: &Connection, item: &str, category: &str, predictions: &[(String, f64)], at: Timestamp, source: Source) -> rusqlite::Result<()> {
    db.execute(SCHEMA, [])?;
    for (tier, p) in predictions {
        db.execute(
            "INSERT OR REPLACE INTO tier_predictions(item, tier, category, p, at, source) VALUES (?1,?2,?3,?4,?5,?6)",
            params![item, tier_key(tier), category, p.clamp(0.0, 1.0), at.iso(), source.name()],
        )?;
    }
    Ok(())
}

/// One category's first runs scored against their stored predictions: the Brier score of the predictions and of the
/// category's base rate over strictly earlier first runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Brier {
    pub category: String,
    pub n: usize,
    pub predicted: f64,
    pub base: f64,
}

impl Brier {
    pub fn line(&self, window: &str) -> String {
        format!("tier success {}: Brier {:.3} against base-rate Brier {:.3} (n {}, {window})", self.category, self.predicted, self.base, self.n)
    }
}

/// The first runs ended since `since` whose Item holds a prediction for their tier made before they started, scored
/// per category with the escalated agents as failures.
pub fn score(db: &Connection, since: &str) -> rusqlite::Result<Vec<Brier>> {
    score_with(db, &escalated(db)?, since)
}

pub fn score_with(db: &Connection, escalated: &BTreeSet<String>, since: &str) -> rusqlite::Result<Vec<Brier>> {
    let stored: Option<String> = db.query_row("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'tier_predictions'", [], |row| row.get(0)).optional()?;
    if stored.is_none() {
        return Ok(Vec::new());
    }
    let seconds = |text: &str| Timestamp::parse_loose(text).map(Timestamp::unix_seconds);
    let mut predictions: BTreeMap<(String, String), Vec<(i64, f64)>> = BTreeMap::new();
    let mut statement = db.prepare("SELECT item, tier, at, p FROM tier_predictions")?;
    for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, f64>(3)?)))? {
        let (item, tier, at, p) = row?;
        if let Some(at) = seconds(&at) {
            predictions.entry((item, tier_key(&tier))).or_default().push((at, p));
        }
    }
    let mut firsts: Vec<First> = raw_runs(db, "follows IS NULL AND outcome != 'split'", &[])?
        .into_iter()
        .filter(|raw| !raw.unjudged())
        .filter_map(|raw| {
            Some(First {
                category: raw.cells[0].clone(),
                started: seconds(raw.started.as_deref()?)?,
                ended: seconds(&raw.ended)?,
                ok: raw.succeeded(escalated.contains(&raw.agent)),
                tier: tier_key(&raw.tier),
                item: raw.item.filter(|item| !item.is_empty()),
            })
        })
        .collect();
    firsts.sort_by_key(|first| first.ended);
    let since = seconds(since).unwrap_or(i64::MIN);
    let mut squares: BTreeMap<String, (usize, f64, f64)> = BTreeMap::new();
    for first in firsts.iter().filter(|first| first.ended >= since) {
        let Some(item) = first.item.clone() else { continue };
        let made = predictions.get(&(item, first.tier.clone())).into_iter().flatten().filter(|(at, _)| *at <= first.started);
        let Some((_, p)) = made.max_by_key(|(at, _)| *at) else { continue };
        let earlier: Vec<bool> = firsts.iter().filter(|other| other.category == first.category && other.ended < first.started).map(|other| other.ok).collect();
        if earlier.is_empty() {
            continue;
        }
        let base = earlier.iter().filter(|ok| **ok).count() as f64 / earlier.len() as f64;
        let outcome = if first.ok { 1.0 } else { 0.0 };
        let sum = squares.entry(first.category.clone()).or_default();
        *sum = (sum.0 + 1, sum.1 + (p - outcome).powi(2), sum.2 + (base - outcome).powi(2));
    }
    Ok(squares.into_iter().map(|(category, (n, predicted, base))| Brier { category, n, predicted: predicted / n as f64, base: base / n as f64 }).collect())
}

/// A first run as the score reads it, its times in unix seconds.
struct First {
    category: String,
    item: Option<String>,
    tier: String,
    started: i64,
    ended: i64,
    ok: bool,
}
