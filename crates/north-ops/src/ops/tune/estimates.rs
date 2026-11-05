//! Each estimator against what its runs came to: per tier and category over the week, the median
//! and p90 ratio of actual to estimated minutes and cost and the share of runs inside their
//! estimate, and each cell whose median ratio drifted out of the calibrated band as the north issue
//! a tick files for it.
use crate::ledger::EstimatedRun;
use crate::ledger::closed::percentile;
use std::collections::BTreeMap;

/// The band of median actual-to-estimate ratios inside which an estimator counts as calibrated.
pub const CALIBRATED: (f64, f64) = (0.67, 1.5);
/// Estimated runs a ratio needs before its drift files an issue, so one odd run files nothing.
pub const DRIFT_MIN_RUNS: usize = 3;

/// How the actual figures of a cell's runs compare with their estimates.
#[derive(Clone, Debug, PartialEq)]
pub struct Ratios {
    pub n: usize,
    pub median: f64,
    pub p90: f64,
    pub inside: f64,
}

impl Ratios {
    fn of(pairs: impl Iterator<Item = (f64, f64)>) -> Option<Self> {
        let mut ratios: Vec<f64> = pairs.filter(|(_, estimate)| *estimate > 0.0).map(|(actual, estimate)| actual / estimate).collect();
        let n = ratios.len();
        (n > 0).then(|| Self { n, inside: ratios.iter().filter(|ratio| **ratio <= 1.0).count() as f64 / n as f64, median: percentile(&mut ratios, 50), p90: percentile(&mut ratios, 90) })
    }

    fn drifted(&self) -> bool {
        self.n >= DRIFT_MIN_RUNS && !(CALIBRATED.0..=CALIBRATED.1).contains(&self.median)
    }

    fn shown(&self) -> String {
        format!("median {:.2}x p90 {:.2}x inside {:.0}% (n {})", self.median, self.p90, self.inside * 100.0, self.n)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub tier: String,
    pub category: String,
    pub minutes: Ratios,
    pub cost: Option<Ratios>,
}

pub fn calibrations(runs: &[EstimatedRun]) -> Vec<Calibration> {
    let mut cells: BTreeMap<(&str, &str), Vec<&EstimatedRun>> = BTreeMap::new();
    for run in runs {
        cells.entry((run.tier.as_str(), run.category.as_str())).or_default().push(run);
    }
    cells
        .into_iter()
        .filter_map(|((tier, category), runs)| {
            Some(Calibration {
                tier: tier.to_owned(),
                category: category.to_owned(),
                minutes: Ratios::of(runs.iter().map(|run| (run.minutes, run.eta_min)))?,
                cost: Ratios::of(runs.iter().filter_map(|run| Some((run.usd, run.estimated_usd?)))),
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

    /// The issue title, the same every day the cell stays drifted so an open issue holds it.
    pub fn title(&self) -> String {
        format!("Estimates drifted outside {}-{}x of actual in {} {}", CALIBRATED.0, CALIBRATED.1, self.tier, self.category)
    }

    pub fn drift_line(&self) -> String {
        let named: Vec<String> = self.drifts().iter().map(|(name, ratios)| format!("{name} median {:.2}x", ratios.median)).collect();
        format!("drifted {} {}: {} outside {}-{}x", self.tier, self.category, named.join(", "), CALIBRATED.0, CALIBRATED.1)
    }

    pub fn body(&self, at: &str) -> String {
        let named: Vec<String> = self.drifts().iter().map(|(name, ratios)| format!("- {name}: actual over estimate {}", ratios.shown())).collect();
        format!(
            "Category: tooling\nSpec: measured\nScope: module\nSurface: rust\nVerify: local-test\n\n\
             `north tune` on {at}: the median ratio of actual to estimated figures of the runs in {tier} {category} over the last 7 days left the calibrated band {low}-{high}x (each ratio holds at least {DRIFT_MIN_RUNS} runs).\n\n{}\n\n\
             ## Done when\n- [ ] (headless run) The drift is traced to the estimator that set these runs' estimates, and either a fix lands or `north tune` shows the cell's median ratio back inside {low}-{high}x.\n",
            named.join("\n"),
            tier = self.tier,
            category = self.category,
            low = CALIBRATED.0,
            high = CALIBRATED.1,
        )
    }
}
