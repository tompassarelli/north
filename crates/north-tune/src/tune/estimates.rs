//! Each estimator against what its runs came to: per tier and category over the week, the median
//! and p90 ratio of actual to estimated minutes and cost and the share of runs inside their
//! estimate, and the cells whose ratio at the quantile their estimator covers drifted out of the
//! calibrated band, together as the one rolling north issue a tick keeps for them.
use crate::ledger::EstimatedRun;
use crate::ledger::closed::percentile;
use crate::tune::budgets::BUDGET_COVERAGE;
use crate::threads::calibration::Knobs;
use std::collections::BTreeMap;

/// The band of actual-to-estimate ratios, read at the quantile the estimator covers, inside which it counts as calibrated.
pub const CALIBRATED: (f64, f64) = (0.67, 1.5);
/// Estimated runs a ratio needs before its drift counts, so one odd run files nothing.
pub const DRIFT_MIN_RUNS: usize = 3;

/// The tier label prefix of runs on the opencode provider: free models whose spend the Claude price table cannot state, and whose briefs carry the Claude cells' estimates.
const OPENCODE_TIERS: &str = "opencode";

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

    fn drifted(&self) -> bool {
        self.n >= DRIFT_MIN_RUNS && !(CALIBRATED.0..=CALIBRATED.1).contains(&self.covered)
    }

    fn shown(&self) -> String {
        format!("p{} {:.2}x median {:.2}x p90 {:.2}x inside {:.0}% (n {})", self.percent, self.covered, self.median, self.p90, self.inside * 100.0, self.n)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub tier: String,
    pub category: String,
    pub minutes: Ratios,
    pub cost: Option<Ratios>,
}

/// Each cell's ratios; the minutes estimate covers the quantile `eta_coverage` names, the cost estimate the p90 `BUDGET_COVERAGE`.
pub fn calibrations(runs: &[EstimatedRun]) -> Vec<Calibration> {
    let eta_percent = (Knobs::registry().eta_coverage * 100.0).round() as usize;
    let mut cells: BTreeMap<(&str, &str), Vec<&EstimatedRun>> = BTreeMap::new();
    for run in runs.iter().filter(|run| !run.tier.starts_with(OPENCODE_TIERS)) {
        cells.entry((run.tier.as_str(), run.category.as_str())).or_default().push(run);
    }
    cells
        .into_iter()
        .filter_map(|((tier, category), runs)| {
            Some(Calibration {
                tier: tier.to_owned(),
                category: category.to_owned(),
                minutes: Ratios::of(eta_percent, runs.iter().map(|run| (run.minutes, run.eta_min)))?,
                cost: Ratios::of(BUDGET_COVERAGE, runs.iter().filter_map(|run| Some((run.usd, run.estimated_usd?)))),
            })
        })
        .collect()
}

impl Calibration {
    pub fn line(&self) -> String {
        let cost = self.cost.as_ref().map_or_else(|| "n/a".to_owned(), Ratios::shown);
        format!("estimate {} {}: minutes {}; cost {cost}", self.tier, self.category, self.minutes.shown())
    }

    /// The estimators of the cell whose median ratio drifted out of the band, by name.
    fn drifts(&self) -> Vec<(&'static str, &Ratios)> {
        [("minutes", Some(&self.minutes)), ("cost", self.cost.as_ref())].into_iter().filter_map(|(name, ratios)| Some((name, ratios.filter(|ratios| ratios.drifted())?))).collect()
    }

    pub fn drifted(&self) -> bool {
        !self.drifts().is_empty()
    }

    pub fn drift_line(&self) -> String {
        let named: Vec<String> = self.drifts().iter().map(|(name, ratios)| format!("{name} p{} {:.2}x", ratios.percent, ratios.covered)).collect();
        format!("drifted {} {}: {} outside {}-{}x", self.tier, self.category, named.join(", "), CALIBRATED.0, CALIBRATED.1)
    }

    fn section(&self) -> String {
        let named: Vec<String> = self.drifts().iter().map(|(name, ratios)| format!("- {name}: actual over estimate {}", ratios.shown())).collect();
        format!("## {} {}\n\n{}\n\n", self.tier, self.category, named.join("\n"))
    }
}

pub const TITLE: &str = "Estimates drifted outside the calibrated band of actual";

pub fn issue_body(calibrations: &[Calibration]) -> Option<String> {
    let sections: String = calibrations.iter().filter(|calibration| calibration.drifted()).map(Calibration::section).collect();
    (!sections.is_empty()).then(|| {
        format!(
            "`north tune` keeps this issue while any tier and category's ratio of actual to estimated minutes or cost, read at the quantile its estimator covers (the ETA's `eta_coverage`, the Budget's p90), over the last 7 days sits outside {low}-{high}x (each ratio holding at least {DRIFT_MIN_RUNS} runs), and closes it on the first tick none does.\n\nCategory: debugging-unknown-cause\nClass: brief estimates that the ledger's own readings show biased, so every ETA and budget line misleads by the same factor\n\n## Done when\n\n- [ ] Every drifted estimator below is recalibrated from its runs, so each tier and category's covered ratio sits inside {low}-{high}x. Done when: `north tune --dry-run | rg -q 'would file: {TITLE}'; test $? -eq 1`\n\n{sections}",
            low = CALIBRATED.0,
            high = CALIBRATED.1,
        )
    })
}
