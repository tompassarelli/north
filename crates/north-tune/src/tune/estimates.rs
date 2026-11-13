//! Each estimator against what its runs came to: per tier and category over the week, the median
//! and p90 ratio of actual to estimated minutes and cost and the share of runs inside their
//! estimate, beside the same ratio for an estimate replayed from the cell's own runs of the 7 days
//! before each run started. A cell drifted when the estimate in use sat outside the calibrated band
//! at the quantile its estimator covers and the replayed one sat inside it: a stale estimate, kept
//! as the one rolling north issue a tick keeps for them. A cell outside the band under both is
//! noisy, and is counted beside the alarm rather than filed.
use crate::foundation::clock::Timestamp;
use crate::ledger::EstimatedRun;
use crate::ledger::closed::percentile;
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

    fn shown(&self) -> String {
        let replayed = self.replayed.as_ref().map_or_else(|| "none".to_owned(), |replayed| format!("p{} {:.2}x (n {})", replayed.percent, replayed.covered, replayed.n));
        format!("{}; replayed {replayed}", self.stated.shown())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub tier: String,
    pub category: String,
    pub minutes: Reading,
    pub cost: Option<Reading>,
}

#[derive(Default)]
struct Cell<'a> {
    scored: Vec<&'a EstimatedRun>,
    pool: Vec<&'a EstimatedRun>,
}

struct Estimator {
    percent: usize,
    actual: fn(&EstimatedRun) -> f64,
    stated: fn(&EstimatedRun) -> Option<f64>,
}

impl Estimator {
    fn reading(&self, scored: &[&EstimatedRun], pool: &[&EstimatedRun]) -> Option<Reading> {
        let stated = Ratios::of(self.percent, scored.iter().filter_map(|run| Some(((self.actual)(run), (self.stated)(run)?))))?;
        let replayed = Ratios::of(self.percent, scored.iter().filter(|run| (self.stated)(run).is_some_and(|estimate| estimate > 0.0)).filter_map(|run| Some(((self.actual)(run), self.replay(run, pool)?))));
        Some(Reading { stated, replayed })
    }

    fn replay(&self, run: &EstimatedRun, pool: &[&EstimatedRun]) -> Option<f64> {
        let started = run.started?;
        let mut earlier: Vec<f64> = pool.iter().filter(|other| (self.stated)(other).is_some_and(|estimate| estimate > 0.0) && Timestamp::parse(&other.ended).is_ok_and(|ended| (started - REPLAY_SECONDS..started).contains(&ended.unix_seconds()))).map(|other| (self.actual)(other)).collect();
        (earlier.len() >= DRIFT_MIN_RUNS).then(|| percentile(&mut earlier, self.percent))
    }
}

/// Each cell's readings over the week's runs; the minutes estimate covers the quantile `eta_coverage` names, the cost estimate the p90 `BUDGET_COVERAGE`.
pub fn calibrations(runs: &EstimatedRuns) -> Vec<Calibration> {
    let minutes = Estimator { percent: (Knobs::registry().eta_coverage * 100.0).round() as usize, actual: |run| run.minutes, stated: |run| Some(run.eta_min) };
    let cost = Estimator { percent: BUDGET_COVERAGE, actual: |run| run.usd, stated: |run| run.estimated_usd };
    let mut cells: BTreeMap<(&str, &str), Cell> = BTreeMap::new();
    for run in runs.week.iter().filter(|run| !run.tier.starts_with(OPENCODE_TIERS)) {
        cells.entry((run.tier.as_str(), run.category.as_str())).or_default().scored.push(run);
    }
    for run in runs.week.iter().chain(&runs.earlier) {
        if let Some(cell) = cells.get_mut(&(run.tier.as_str(), run.category.as_str())) {
            cell.pool.push(run);
        }
    }
    cells
        .into_iter()
        .filter_map(|((tier, category), cell)| {
            Some(Calibration { tier: tier.to_owned(), category: category.to_owned(), minutes: minutes.reading(&cell.scored, &cell.pool)?, cost: cost.reading(&cell.scored, &cell.pool) })
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

    fn section(&self) -> String {
        let named: Vec<String> = self.named(Verdict::Stale).iter().map(|(name, reading)| format!("- {name}: actual over estimate {}", reading.shown())).collect();
        format!("## {} {}\n\n{}\n\n", self.tier, self.category, named.join("\n"))
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

pub fn issue_body(calibrations: &[Calibration]) -> Option<String> {
    let sections: String = calibrations.iter().filter(|calibration| calibration.drifted()).map(Calibration::section).collect();
    (!sections.is_empty()).then(|| {
        format!(
            "`north tune` keeps this issue while any tier and category's ratio of actual to estimated minutes or cost, read at the quantile its estimator covers (the ETA's `eta_coverage`, the Budget's p90), over the last 7 days sits outside {low}-{high}x (each ratio holding at least {DRIFT_MIN_RUNS} runs) where an estimate replayed from the cell's own runs of the 7 days before each run would have sat inside it, and closes it on the first tick none does.\n\nCategory: debugging-unknown-cause\nClass: brief estimates that the ledger's own readings show biased, so every ETA and budget line misleads by the same factor\n\n## Done when\n\n- [ ] Every drifted estimator below is recalibrated from its runs, so each tier and category's covered ratio sits inside {low}-{high}x. Done when: `north tune --dry-run | rg -q 'would file: {TITLE}'; test $? -eq 1`\n\n{sections}",
            low = CALIBRATED.0,
            high = CALIBRATED.1,
        )
    })
}
