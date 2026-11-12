//! `north threads backtest`: experiment L1 of north#1221, read-only over the ledger. Every learned value that drives a
//! decision is scored on the runs of the 14 days before now, each against what was learned before it started, beside
//! the constant it would replace, refreshed daily from the 7 days before each day: the ETA as a p75 by pinball loss,
//! the budget as a p90, the per-run cost as a median, the tier-success estimate by Brier score against the base rate,
//! and the arm shares by the cost per kept box they buy against equal shares. North tune tells each reading to the
//! graph's skill switch, which every caller of the decision reads.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use super::predictions::score_with;
use super::read::escalated;
use crate::foundation::clock::Timestamp;
use crate::platform::run_roles::worker_where;
use crate::router::settle::{self, Outcome};

const DAY: i64 = 86_400;
const SCORED_DAYS: i64 = 14;
const CONSTANT_DAYS: i64 = 7;
const ARM_DAYS: i64 = 30;
const CELL_MIN: usize = 5;
const LEARNED: [&str; 2] = ["tune", "ledger"];

/// A finished worker run as the backtest reads it, its times in unix seconds.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub item: String,
    pub box_text: String,
    pub tier: String,
    pub category: String,
    pub arm: String,
    pub started: i64,
    pub ended: i64,
    pub minutes: f64,
    pub eta: Option<(f64, String)>,
    pub budget: Option<(f64, String)>,
    pub cost: Option<f64>,
    pub landed: bool,
}

/// One decision's walk-forward reading, named as the skill switch names it: the learned value's mean loss and the
/// constant's on the same runs (lower is better), the runs behind them, the constant's own value on the newest window,
/// and the registered L1 line it is held to, when it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub decision: &'static str,
    pub n: u64,
    pub learned: f64,
    pub constant: f64,
    pub value: Option<f64>,
    pub score: &'static str,
    pub registered: Option<(bool, String)>,
}

fn pinball(tau: f64, actual: f64, quantile: f64) -> f64 {
    if actual >= quantile { tau * (actual - quantile) } else { (1.0 - tau) * (quantile - actual) }
}

fn quantile(values: &mut [f64], q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[((values.len() - 1) as f64 * q).round() as usize])
}

/// The constant's value for runs that started on the day of `at`: the quantile of `measure` over the runs that ended in
/// the `CONSTANT_DAYS` before that day began.
struct Daily<'a> {
    runs: &'a [Run],
    cache: BTreeMap<(i64, u64, u8), Option<f64>>,
}

impl<'a> Daily<'a> {
    fn new(runs: &'a [Run]) -> Self {
        Self { runs, cache: BTreeMap::new() }
    }

    fn at(&mut self, at: i64, q: f64, key: u8, measure: impl Fn(&Run) -> Option<f64>) -> Option<f64> {
        let day = at.div_euclid(DAY) * DAY;
        let runs = self.runs;
        *self.cache.entry((day, q.to_bits(), key)).or_insert_with(|| {
            let mut values: Vec<f64> = runs.iter().filter(|run| run.ended >= day - CONSTANT_DAYS * DAY && run.ended < day).filter_map(&measure).collect();
            quantile(&mut values, q)
        })
    }
}

struct Sums {
    n: u64,
    learned: f64,
    constant: f64,
}

impl Sums {
    fn new() -> Self {
        Self { n: 0, learned: 0.0, constant: 0.0 }
    }

    fn add(&mut self, learned: f64, constant: f64) {
        self.n += 1;
        self.learned += learned;
        self.constant += constant;
    }

    fn reading(&self, decision: &'static str, value: Option<f64>, score: &'static str, registered: Option<(bool, String)>) -> Reading {
        let n = self.n.max(1) as f64;
        Reading { decision, n: self.n, learned: self.learned / n, constant: self.constant / n, value, score, registered }
    }
}

fn scored(runs: &[Run], now: i64) -> impl Iterator<Item = &Run> {
    runs.iter().filter(move |run| run.started >= now - SCORED_DAYS * DAY && run.started < now)
}

fn learned(source: &(f64, String)) -> Option<f64> {
    (source.0 > 0.0 && LEARNED.contains(&source.1.as_str())).then_some(source.0)
}

/// The stated ETA against the window's p75 of wall minutes, both read by the pinball loss at 0.75, and L1's line:
/// inside the ETA on 70% to 80% of runs with actual over ETA p90 at most 2.5, on at least 300 runs.
pub fn eta(runs: &[Run], now: i64) -> Reading {
    let mut daily = Daily::new(runs);
    let mut sums = Sums::new();
    let (mut inside, mut ratios) = (0_u64, Vec::new());
    for run in scored(runs, now) {
        let (Some(stated), Some(constant)) = (run.eta.as_ref().and_then(learned), daily.at(run.started, 0.75, 0, |run| Some(run.minutes))) else { continue };
        sums.add(pinball(0.75, run.minutes, stated), pinball(0.75, run.minutes, constant.max(1.0)));
        inside += u64::from(run.minutes <= stated);
        ratios.push(run.minutes / stated);
    }
    let share = inside as f64 / sums.n.max(1) as f64;
    let p90 = quantile(&mut ratios, 0.9).unwrap_or(f64::NAN);
    let passes = sums.n >= 300 && (0.70..=0.80).contains(&share) && p90 <= 2.5;
    let value = daily.at(now, 0.75, 0, |run| Some(run.minutes)).map(f64::ceil);
    sums.reading("eta", value, "pinball loss at p75, minutes", Some((passes, format!("{:.0}% inside the ETA, actual/ETA p90 {p90:.1}", share * 100.0))))
}

/// The stated budget against the window's p90 of run cost, read by the pinball loss at 0.9, and L1's line: 5% to 15%
/// of runs over their budget on at least 300 runs.
pub fn budget(runs: &[Run], now: i64) -> Reading {
    let mut daily = Daily::new(runs);
    let mut sums = Sums::new();
    let mut over = 0_u64;
    for run in scored(runs, now) {
        let (Some(stated), Some(cost), Some(constant)) = (run.budget.as_ref().and_then(learned), run.cost, daily.at(run.started, 0.9, 1, |run| run.cost)) else { continue };
        sums.add(pinball(0.9, cost, stated), pinball(0.9, cost, constant));
        over += u64::from(cost > stated);
    }
    let share = over as f64 / sums.n.max(1) as f64;
    let passes = sums.n >= 300 && (0.05..=0.15).contains(&share);
    sums.reading("budget", daily.at(now, 0.9, 1, |run| run.cost), "pinball loss at p90, USD", Some((passes, format!("{:.0}% of runs over budget", share * 100.0))))
}

/// The cell's median run cost (category and tier, at least five runs) against the window's median, by absolute error.
pub fn cost(runs: &[Run], now: i64) -> Reading {
    let mut daily = Daily::new(runs);
    let mut cells: BTreeMap<(i64, &str, &str), Option<f64>> = BTreeMap::new();
    let mut sums = Sums::new();
    for run in scored(runs, now) {
        let (Some(cost), Some(constant)) = (run.cost, daily.at(run.started, 0.5, 2, |run| run.cost)) else { continue };
        let day = run.started.div_euclid(DAY) * DAY;
        let cell = *cells.entry((day, &run.category, &run.tier)).or_insert_with(|| {
            let mut values: Vec<f64> = runs.iter().filter(|other| other.category == run.category && other.tier == run.tier && other.ended >= day - CONSTANT_DAYS * DAY && other.ended < day).filter_map(|other| other.cost).collect();
            (values.len() >= CELL_MIN).then(|| quantile(&mut values, 0.5)).flatten()
        });
        let Some(cell) = cell else { continue };
        sums.add((cost - cell).abs(), (cost - constant).abs());
    }
    sums.reading("cost", daily.at(now, 0.5, 2, |run| run.cost), "absolute error of the median, USD", None)
}

/// The stored tier-success predictions of first runs against each category's earlier base rate, by Brier score, and
/// L1's line: at least 0.01 below the base rate's on at least 500 first runs.
pub fn tier_success(db: &Connection, now: i64) -> rusqlite::Result<Reading> {
    let since = Timestamp::from_unix_seconds(now - SCORED_DAYS * DAY).iso();
    let briers = score_with(db, &escalated(db)?, &since)?;
    let mut sums = Sums::new();
    for brier in &briers {
        sums.n += brier.n as u64;
        sums.learned += brier.predicted * brier.n as f64;
        sums.constant += brier.base * brier.n as f64;
    }
    let n = sums.n.max(1) as f64;
    let (learned, constant) = (sums.learned / n, sums.constant / n);
    let passes = sums.n >= 500 && learned <= constant - 0.01;
    Ok(sums.reading("tier-success", None, "Brier score", Some((passes, format!("Brier {learned:.3} against {constant:.3}")))))
}

fn boxes<'a>(runs: impl Iterator<Item = &'a Run>) -> BTreeMap<(String, String), BTreeMap<String, Vec<Outcome>>> {
    let mut grouped: BTreeMap<(String, String, String, String), (f64, bool)> = BTreeMap::new();
    for run in runs.filter(|run| !run.arm.is_empty() && !run.item.is_empty()) {
        let entry = grouped.entry((run.category.clone(), run.arm.clone(), run.item.clone(), run.box_text.clone())).or_default();
        entry.0 += run.cost.unwrap_or(0.0);
        entry.1 |= run.landed;
    }
    let mut experiments: BTreeMap<(String, String), BTreeMap<String, Vec<Outcome>>> = BTreeMap::new();
    for ((category, arm, ..), (usd, landed)) in grouped {
        experiments.entry((category, settle::family(&arm).to_owned())).or_default().entry(arm).or_default().push(Outcome { usd, closed: landed, kept: landed });
    }
    experiments
}

/// The shares the 30 days before the window give each experiment's arms, against equal shares, scored by the cost per
/// kept box each would have bought on the window's boxes; an experiment with an arm that kept no box there is skipped.
pub fn arm_shares(runs: &[Run], now: i64) -> Reading {
    let start = now - SCORED_DAYS * DAY;
    let before = boxes(runs.iter().filter(|run| run.started >= start - ARM_DAYS * DAY && run.started < start));
    let window = boxes(scored(runs, now));
    let mut sums = Sums::new();
    for (experiment, arms) in &window {
        let names: BTreeSet<&String> = arms.keys().collect();
        if names.len() < 2 {
            continue;
        }
        let Some(costs) = names.iter().map(|arm| settle::usd_per_kept(&arms[*arm])).collect::<Option<Vec<f64>>>() else { continue };
        let empty = BTreeMap::new();
        let prior = before.get(experiment).unwrap_or(&empty);
        let held: Vec<&[Outcome]> = names.iter().map(|arm| prior.get(*arm).map_or(&[][..], Vec::as_slice)).collect();
        let (shares, _) = settle::shares(&held, &format!("{} {}", experiment.0, experiment.1));
        let total: f64 = shares.iter().sum();
        let boxes = arms.values().map(Vec::len).sum::<usize>() as f64;
        let learned = shares.iter().zip(&costs).map(|(share, cost)| share / total * cost).sum::<f64>();
        let equal = costs.iter().sum::<f64>() / costs.len() as f64;
        sums.n += boxes as u64;
        sums.learned += learned * boxes;
        sums.constant += equal * boxes;
    }
    sums.reading("arm-shares", None, "USD per kept box", None)
}

/// Every finished worker run that started in the `SCORED_DAYS + ARM_DAYS` before `now`.
pub fn runs(db: &Connection, now: i64) -> rusqlite::Result<Vec<Run>> {
    let since = Timestamp::from_unix_seconds(now - (SCORED_DAYS + ARM_DAYS) * DAY).iso();
    let query = format!(
        "SELECT COALESCE(item, ''), COALESCE(box, ''), tier, category, COALESCE(arm, ''), started, ended, minutes, eta_min, COALESCE(eta_source, ''), budget_usd, COALESCE(budget_source, ''), cost_usd, \
         (COALESCE(landed, 0) = 1 OR landed_sha IS NOT NULL) FROM runs WHERE {} AND outcome NOT IN ('running', 'idle') AND ended != '' AND started >= ?1",
        worker_where()
    );
    let seconds = |text: &str| Timestamp::parse_loose(text).map(Timestamp::unix_seconds);
    let mut statement = db.prepare(&query)?;
    let rows = statement.query_map([&since], |row| {
        let started: Option<String> = row.get(5)?;
        let ended: String = row.get(6)?;
        let eta: Option<f64> = row.get(8)?;
        let budget: Option<f64> = row.get(10)?;
        Ok(started.as_deref().and_then(seconds).zip(seconds(&ended)).map(|(started, ended)| Run {
            item: row.get(0).unwrap_or_default(),
            box_text: row.get(1).unwrap_or_default(),
            tier: row.get(2).unwrap_or_default(),
            category: row.get(3).unwrap_or_default(),
            arm: row.get(4).unwrap_or_default(),
            started,
            ended,
            minutes: row.get::<_, f64>(7).unwrap_or(0.0),
            eta: eta.map(|eta| (eta, row.get(9).unwrap_or_default())),
            budget: budget.map(|budget| (budget, row.get(11).unwrap_or_default())),
            cost: row.get(12).unwrap_or(None),
            landed: row.get::<_, i64>(13).unwrap_or(0) == 1,
        }))
    })?;
    Ok(rows.filter_map(|row| row.ok().flatten()).collect())
}

pub fn backtest(db: &Connection, now: i64) -> rusqlite::Result<Vec<Reading>> {
    let runs = runs(db, now)?;
    Ok(vec![eta(&runs, now), budget(&runs, now), cost(&runs, now), tier_success(db, now)?, arm_shares(&runs, now)])
}

/// The backtest over the ledger at `database`, opened read-only.
pub fn backtest_in(database: &std::path::Path, now: i64) -> Result<Vec<Reading>, String> {
    let db = Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("ledger {}: {why}", database.display()))?;
    backtest(&db, now).map_err(|why| format!("backtest over {}: {why}", database.display()))
}
