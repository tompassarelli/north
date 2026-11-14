//! Each estimator against what its runs came to: per tier and category over the week, the median
//! and p90 ratio of actual to estimated minutes per run and cost per box and the share inside their
//! estimate, beside the same ratio for an estimate replayed from the cell's own runs of the 7 days
//! before each run started. A cell drifted when the estimate in use sat outside the calibrated band
//! at the quantile its estimator covers and the replayed one sat inside it: a stale estimate,
//! which the tick tells the proxy of whenever the set of them changes. A cell outside the band under
//! both is noisy, and is counted beside the alarm rather than told.
use crate::foundation::clock::Timestamp;
use crate::ledger::EstimatedRun;
use crate::ledger::closed::percentile;
use crate::dispatch::floored_p90;
use crate::tune::budgets::BUDGET_COVERAGE;
use crate::threads::calibration::Knobs;
use std::collections::BTreeMap;

/// The band of actual-to-estimate ratios, read at the quantile the estimator covers, inside which it counts as calibrated.
pub const CALIBRATED: (f64, f64) = (0.67, 1.5);
/// Estimated runs a ratio needs before its drift counts, so one odd run files nothing; also the
/// earlier runs a replayed estimate needs, and the replayed runs a replayed ratio needs.
pub const DRIFT_MIN_RUNS: usize = 3;
const REPLAY_SECONDS: i64 = 7 * 86_400;

/// The tier label prefix of runs on the opencode provider: free models whose spend the Claude price table cannot state, and whose briefs carry the Claude cells' estimates.
const OPENCODE_TIERS: &str = "opencode";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EstimatedRuns {
    pub week: Vec<EstimatedRun>,
    pub earlier: Vec<EstimatedRun>,
}

/// How the actual figures of a cell's runs compare with their estimates.
#[derive(Clone, Debug, PartialEq)]
pub struct Ratios {
    pub n: usize,
    pub percent: usize,
    pub covered: f64,
    pub median: f64,
    pub p90: f64,
    pub inside: f64,
}

impl Ratios {
    /// The ratios of `pairs`, read at `percent`, the quantile of actual the estimator that stated the estimates covers.
    fn of(percent: usize, pairs: impl Iterator<Item = (f64, f64)>) -> Option<Self> {
        let mut ratios: Vec<f64> = pairs.filter(|(_, estimate)| *estimate > 0.0).map(|(actual, estimate)| actual / estimate).collect();
        let n = ratios.len();
        (n > 0).then(|| Self { n, percent, covered: percentile(&mut ratios, percent), inside: ratios.iter().filter(|ratio| **ratio <= 1.0).count() as f64 / n as f64, median: percentile(&mut ratios, 50), p90: percentile(&mut ratios, 90) })
    }

    fn outside(&self) -> bool {
        self.n >= DRIFT_MIN_RUNS && !(CALIBRATED.0..=CALIBRATED.1).contains(&self.covered)
    }

    fn shown(&self) -> String {
        format!("p{} {:.2}x median {:.2}x p90 {:.2}x inside {:.0}% (n {})", self.percent, self.covered, self.median, self.p90, self.inside * 100.0, self.n)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub stated: Ratios,
    pub replayed: Option<Ratios>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Verdict {
    Inside,
    Stale,
    Noisy,
    Unreplayed,
}

impl Reading {
    fn verdict(&self) -> Verdict {
        if !self.stated.outside() {
            return Verdict::Inside;
        }
        match self.replayed.as_ref().filter(|replayed| replayed.n >= DRIFT_MIN_RUNS) {
            Some(replayed) if replayed.outside() => Verdict::Noisy,
            Some(_) => Verdict::Stale,
            None => Verdict::Unreplayed,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub tier: String,
    pub category: String,
    pub minutes: Reading,
    pub cost: Option<Reading>,
}

struct Sample<'a> {
    tier: &'a str,
    category: &'a str,
    actual: f64,
    stated: Option<f64>,
    started: Option<i64>,
    ended: Option<i64>,
    week: bool,
}

#[derive(Default)]
struct Cell<'a> {
    scored: Vec<&'a Sample<'a>>,
    pool: Vec<&'a Sample<'a>>,
}

fn cells<'a>(samples: &'a [Sample<'a>]) -> BTreeMap<(&'a str, &'a str), Cell<'a>> {
    let mut cells: BTreeMap<(&str, &str), Cell> = BTreeMap::new();
    for sample in samples.iter().filter(|sample| sample.week && !sample.tier.starts_with(OPENCODE_TIERS)) {
        cells.entry((sample.tier, sample.category)).or_default().scored.push(sample);
    }
    for ((tier, category), cell) in &mut cells {
        cell.pool.extend(samples.iter().filter(|sample| sample.category == *category && sample.tier == *tier));
    }
    cells
}

fn dated(runs: &EstimatedRuns) -> impl Iterator<Item = (&EstimatedRun, bool)> {
    runs.week.iter().map(|run| (run, true)).chain(runs.earlier.iter().map(|run| (run, false)))
}

fn ended(run: &EstimatedRun) -> Option<i64> {
    Timestamp::parse(&run.ended).ok().map(Timestamp::unix_seconds)
}

fn minutes(runs: &EstimatedRuns) -> Vec<Sample<'_>> {
    dated(runs).map(|(run, week)| Sample { tier: &run.tier, category: &run.category, actual: run.minutes, stated: Some(run.eta_min), started: run.started, ended: ended(run), week }).collect()
}

type BoxKey<'a> = (&'a str, &'a str, Result<&'a str, usize>);

/// A cell's runs collapsed by item into boxes, each its runs' spend against the first Budget its briefs stated, as a brief's Budget prices a box.
fn boxes(runs: &EstimatedRuns) -> Vec<Sample<'_>> {
    let mut grouped: BTreeMap<BoxKey, Vec<(&EstimatedRun, bool)>> = BTreeMap::new();
    for (index, (run, week)) in dated(runs).enumerate() {
        grouped.entry((&run.tier, &run.category, run.item.as_deref().ok_or(index))).or_default().push((run, week));
    }
    grouped
        .into_iter()
        .map(|((tier, category, _), mut runs)| {
            runs.sort_by_key(|(run, _)| run.started.unwrap_or(i64::MAX));
            Sample { tier, category, actual: runs.iter().map(|(run, _)| run.usd).sum(), stated: runs.iter().find_map(|(run, _)| run.estimated_usd), started: runs.first().and_then(|(run, _)| run.started), ended: runs.iter().filter_map(|(run, _)| ended(run)).max(), week: runs.iter().any(|(_, week)| *week) }
        })
        .collect()
}

struct Estimator {
    percent: usize,
    floored: bool,
}

impl Estimator {
    fn reading(&self, cell: &Cell, every: &[Sample]) -> Option<Reading> {
        let stated = Ratios::of(self.percent, cell.scored.iter().filter_map(|sample| Some((sample.actual, sample.stated?))))?;
        let replayed = Ratios::of(self.percent, cell.scored.iter().filter(|sample| sample.stated.is_some_and(|estimate| estimate > 0.0)).filter_map(|sample| Some((sample.actual, self.replay(sample, &cell.pool, every)?))));
        Some(Reading { stated, replayed })
    }

    fn replay(&self, sample: &Sample, pool: &[&Sample], every: &[Sample]) -> Option<f64> {
        let started = sample.started?;
        let before = |other: &Sample| other.ended.is_some_and(|ended| (started - REPLAY_SECONDS..started).contains(&ended));
        let mut earlier: Vec<f64> = pool.iter().filter(|other| other.stated.is_some_and(|estimate| estimate > 0.0) && before(other)).map(|other| other.actual).collect();
        if earlier.len() < DRIFT_MIN_RUNS {
            return None;
        }
        let replayed = percentile(&mut earlier, self.percent);
        if !self.floored {
            return Some(replayed);
        }
        let mut across: Vec<f64> = every.iter().filter(|other| before(other)).map(|other| other.actual).collect();
        Some(floored_p90(replayed, (!across.is_empty()).then(|| percentile(&mut across, 50))))
    }
}

/// Each cell's readings over the week's runs; the minutes estimate covers the quantile `eta_coverage` names per run, the cost estimate the p90 `BUDGET_COVERAGE` per box, raised to the median box across cells as a brief's Budget is.
pub fn calibrations(runs: &EstimatedRuns) -> Vec<Calibration> {
    let minutes_estimator = Estimator { percent: (Knobs::registry().eta_coverage * 100.0).round() as usize, floored: false };
    let cost_estimator = Estimator { percent: BUDGET_COVERAGE, floored: true };
    let (minutes, boxes) = (minutes(runs), boxes(runs));
    let costs = cells(&boxes);
    cells(&minutes)
        .into_iter()
        .filter_map(|((tier, category), cell)| {
            Some(Calibration { tier: tier.to_owned(), category: category.to_owned(), minutes: minutes_estimator.reading(&cell, &minutes)?, cost: costs.get(&(tier, category)).and_then(|cell| cost_estimator.reading(cell, &boxes)) })
        })
        .collect()
}

impl Calibration {
    pub fn line(&self) -> String {
        let cost = self.cost.as_ref().map_or_else(|| "n/a".to_owned(), |cost| cost.stated.shown());
        format!("estimate {} {}: minutes {}; cost {cost}", self.tier, self.category, self.minutes.stated.shown())
    }

    fn named(&self, verdict: Verdict) -> Vec<(&'static str, &Reading)> {
        [("minutes", Some(&self.minutes)), ("cost", self.cost.as_ref())].into_iter().filter_map(|(name, reading)| Some((name, reading.filter(|reading| reading.verdict() == verdict)?))).collect()
    }

    pub fn drifted(&self) -> bool {
        !self.named(Verdict::Stale).is_empty()
    }

    fn summed(&self, verdict: Verdict) -> String {
        let named: Vec<String> = self.named(verdict).iter().filter_map(|(name, reading)| Some(format!("{name} p{} {:.2}x, replayed {:.2}x", reading.stated.percent, reading.stated.covered, reading.replayed.as_ref()?.covered))).collect();
        named.join("; ")
    }

    pub fn drift_line(&self) -> String {
        format!("drifted {} {}: {} outside {}-{}x", self.tier, self.category, self.summed(Verdict::Stale), CALIBRATED.0, CALIBRATED.1)
    }
}

pub fn noise_lines(calibrations: &[Calibration]) -> Vec<String> {
    let noisy: Vec<&Calibration> = calibrations.iter().filter(|calibration| !calibration.named(Verdict::Noisy).is_empty()).collect();
    let unreplayed: Vec<String> = calibrations.iter().filter(|calibration| !calibration.named(Verdict::Unreplayed).is_empty()).map(|calibration| format!("{} {}", calibration.tier, calibration.category)).collect();
    let mut lines = vec![format!("outside the band under every estimator: {} cells", noisy.len())];
    lines.extend(noisy.iter().map(|calibration| format!("  noisy {} {}: {}", calibration.tier, calibration.category, calibration.summed(Verdict::Noisy))));
    lines.push(format!("  outside the band with too few earlier runs to replay, neither drifted nor counted: {} cells{}", unreplayed.len(), if unreplayed.is_empty() { String::new() } else { format!(" ({})", unreplayed.join(", ")) }));
    lines
}

pub const TITLE: &str = "Estimates drifted outside the calibrated band of actual";
