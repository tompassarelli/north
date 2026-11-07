//! Points per dollar: how far an account's five-hour and week windows rise for each priced dollar of fleet turns.
use std::collections::BTreeMap;

use rusqlite::Connection;

use super::forward::{Figure, Row};
use super::quantile;
use super::read::{every_run, raw_runs};
use crate::foundation::clock::Timestamp;
use crate::spend::Turn;
use crate::usage::decide::world::{Rate, Rates};

pub const ACCOUNT_RATE_HOURS: usize = 20;
pub const TURN_MIN_RUNS: usize = 20;
const POOLED: &str = "pooled";
const WINDOWS: [(&str, &str); 2] = [("five_hour", "session"), ("week", "weekly_all")];

pub struct Sample {
    pub account: String,
    pub window: &'static str,
    pub at: String,
    pub used_pct: f64,
    pub resets_at: String,
}

/// One run's API-equivalent dollars on an account, spent between `started` and `ended`.
pub struct Cost {
    pub account: String,
    pub started: String,
    pub ended: String,
    pub usd: f64,
}

fn hour(at: &str) -> String {
    at.chars().take(13).collect()
}

fn seconds(at: &str) -> Option<i64> {
    Timestamp::parse(at).ok().map(Timestamp::unix_seconds)
}

/// The reset moment to the minute: the provider reports one reset a second apart (02:00:00Z and 01:59:59Z) between reads.
fn reset(at: &str) -> Option<i64> {
    seconds(at).map(|seconds| (seconds + 1).div_euclid(60))
}

/// The dollars of `cost` in each hour it ran, in proportion to the time it ran in that hour.
fn by_hour(cost: &Cost) -> Vec<(String, f64)> {
    let (Some(start), Some(end)) = (seconds(&cost.started), seconds(&cost.ended)) else { return Vec::new() };
    if end <= start {
        return vec![(hour(&cost.started), cost.usd)];
    }
    (start.div_euclid(3600)..=(end - 1).div_euclid(3600))
        .map(|slot| {
            let overlap = end.min((slot + 1) * 3600) - start.max(slot * 3600);
            (hour(&Timestamp::from_unix_seconds(slot * 3600).iso()), cost.usd * overlap as f64 / (end - start) as f64)
        })
        .collect()
}

fn rises(samples: &[Sample]) -> BTreeMap<(String, &'static str, String), f64> {
    let mut by_window: BTreeMap<(&str, &'static str), Vec<&Sample>> = BTreeMap::new();
    for sample in samples {
        by_window.entry((&sample.account, sample.window)).or_default().push(sample);
    }
    let mut rise = BTreeMap::new();
    for ((account, window), mut series) in by_window {
        series.sort_by(|x, y| x.at.cmp(&y.at));
        for pair in series.windows(2) {
            let step = pair[1].used_pct - pair[0].used_pct;
            if reset(&pair[0].resets_at) == reset(&pair[1].resets_at) && step > 0.0 {
                *rise.entry((account.to_owned(), window, hour(&pair[1].at))).or_default() += step;
            }
        }
    }
    rise
}

/// One row per account and window, and a pooled row per window: cell `ACCOUNT WINDOW`, n the hours holding both a rise and priced turns, value points per dollar.
pub fn measure(samples: &[Sample], costs: &[Cost]) -> Vec<Row> {
    let mut spent: BTreeMap<(&str, String), f64> = BTreeMap::new();
    for cost in costs {
        for (at, usd) in by_hour(cost) {
            *spent.entry((&cost.account, at)).or_default() += usd;
        }
    }
    let mut totals: BTreeMap<(String, &'static str), (f64, f64, usize)> = BTreeMap::new();
    for ((account, window, at), rise) in rises(samples) {
        let Some(usd) = spent.get(&(account.as_str(), at)).copied().filter(|usd| *usd > 0.0) else { continue };
        for owner in [account, POOLED.to_owned()] {
            let total = totals.entry((owner, window)).or_default();
            total.0 += rise;
            total.1 += usd;
            total.2 += 1;
        }
    }
    totals
        .into_iter()
        .map(|((owner, window), (points, usd, hours))| Row { cell: format!("{owner} {window}"), n: hours, p25: None, median: None, p75: None, value: Some(Figure::Real(points / usd)) })
        .collect()
}

pub fn calibrate_rates(db: &Connection, start: &str, end: &str) -> rusqlite::Result<Vec<Row>> {
    let present = |table: &str| db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |row| row.get::<_, i64>(0)).map(|count| count > 0);
    let mut samples = Vec::new();
    let windows = if present("usage")? { WINDOWS.as_slice() } else { &[] };
    for &(window, name) in windows {
        let mut statement = db.prepare("SELECT account, ts, used_pct, COALESCE(resets_at, '') FROM usage WHERE provider = 'claude' AND kind = 'window' AND name = ?1 AND used_pct IS NOT NULL AND ts >= ?2 AND ts < ?3")?;
        let found = statement.query_map([name, start, end], |row| Ok(Sample { account: row.get(0)?, window, at: row.get(1)?, used_pct: row.get(2)?, resets_at: row.get(3)? }))?;
        for sample in found {
            samples.push(sample?);
        }
    }
    let costs = every_run(db, "started IS NOT NULL AND ended >= ?1 AND started < ?2 AND account IS NOT NULL", &[&start, &end])?
        .into_iter()
        .filter_map(|run| Some(Cost { account: run.account?.trim_start_matches("claude-").to_owned(), started: run.started?, ended: run.ended, usd: run.api_usd? }))
        .collect::<Vec<_>>();
    Ok(measure(&samples, &costs))
}

/// An account's rates from the newest `rate` rows: its own when measured over at least `ACCOUNT_RATE_HOURS` hours, else the pooled row, else None.
pub fn rates_of(db: &Connection, account: &str) -> rusqlite::Result<Option<Rates>> {
    let newest = |owner: &str, window: &str| -> rusqlite::Result<Option<(f64, usize)>> {
        let mut statement = db.prepare("SELECT value, n FROM calibration WHERE kind = 'rate' AND cell = ?1 AND value IS NOT NULL ORDER BY at DESC, rowid DESC LIMIT 1")?;
        let mut found = statement.query_map([format!("{owner} {window}")], |row| Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)? as usize)))?;
        found.next().transpose()
    };
    let pick = |window: &str| -> rusqlite::Result<Option<Rate>> {
        if let Some((points_per_usd, n)) = newest(account, window)?
            && n >= ACCOUNT_RATE_HOURS
        {
            return Ok(Some(Rate::Account { points_per_usd, n: n as u32 }));
        }
        Ok(newest(POOLED, window)?.map(|(points_per_usd, n)| Rate::Pooled { points_per_usd, n: n as u32 }))
    };
    Ok(match (pick("five_hour")?, pick("week")?) {
        (Some(five_hour), Some(week)) => Some(Rates { five_hour, week }),
        _ => None,
    })
}

/// The p90 API-equivalent dollars of one worker run in the ledger since `since`, which prices a worker turn on any account.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurnCost {
    pub p90_usd: f64,
    pub runs: usize,
}

impl TurnCost {
    /// None under `TURN_MIN_RUNS` priced runs, too few for a p90.
    pub fn read(db: &Connection, since: &str) -> rusqlite::Result<Option<Self>> {
        let costs: Vec<f64> = raw_runs(db, "outcome != 'running' AND ended >= ?1", &[&since])?.into_iter().filter_map(|run| run.api_usd).collect();
        Ok(if costs.len() < TURN_MIN_RUNS { None } else { quantile(&costs, 0.9).map(|p90_usd| Self { p90_usd, runs: costs.len() }) })
    }

    /// The turn on `account` (its label, such as `a`): this cost at the account's measured points per dollar. None without a measured rate.
    pub fn on(&self, db: &Connection, account: &str) -> rusqlite::Result<Option<Turn>> {
        Ok(rates_of(db, account)?.map(|rates| Turn::measured(self.p90_usd, self.runs, rates.five_hour.points_per_usd(), rates.week.points_per_usd())))
    }
}
