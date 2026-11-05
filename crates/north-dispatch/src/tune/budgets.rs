//! Each run's Budget (its expected spend E, stored at launch) against what it spent: per category
//! over the week, the runs carrying a stored Budget, the share of them over it, the p50 and p90 of
//! actual over E, and the budget multiplier that p90 sets once the category holds enough runs.
use crate::ledger::EstimatedRun;
use crate::ledger::closed::percentile;
use crate::dispatch::BUDGET_TIMES;
use std::collections::BTreeMap;

/// Runs carrying a stored Budget a category needs before its p90 sets the multiplier.
pub const SCORED_MIN_RUNS: usize = 30;

/// How a category's runs spent against the Budget each launched with.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    pub category: String,
    pub n: usize,
    pub over: f64,
    pub p50: f64,
    pub p90: f64,
    /// How far the category's median Budget sat above its median spend: the multiplier its
    /// Budgets were set with, read back from the runs themselves.
    pub set_with: f64,
}

pub fn scores(runs: &[EstimatedRun]) -> Vec<Score> {
    let mut categories: BTreeMap<&str, Vec<(f64, f64)>> = BTreeMap::new();
    for run in runs {
        if let Some(budget) = run.estimated_usd.filter(|budget| *budget > 0.0) {
            categories.entry(run.category.as_str()).or_default().push((run.usd, budget));
        }
    }
    categories
        .into_iter()
        .map(|(category, pairs)| {
            let n = pairs.len();
            let mut ratios: Vec<f64> = pairs.iter().map(|(usd, budget)| usd / budget).collect();
            let mut budgets: Vec<f64> = pairs.iter().map(|(_, budget)| *budget).collect();
            let mut spent: Vec<f64> = pairs.iter().map(|(usd, _)| *usd).collect();
            let spent = percentile(&mut spent, 50);
            Score {
                category: category.to_owned(),
                n,
                over: ratios.iter().filter(|ratio| **ratio > 1.0).count() as f64 / n as f64,
                p50: percentile(&mut ratios, 50),
                p90: percentile(&mut ratios, 90),
                set_with: if spent > 0.0 { percentile(&mut budgets, 50) / spent } else { BUDGET_TIMES },
            }
        })
        .collect()
}

impl Score {
    /// The multiplier on the median closed box that would have covered the p90 run: the p90 of
    /// actual over E times the multiplier the Budgets were set with, never below 1; None while the
    /// category holds fewer than `SCORED_MIN_RUNS` runs carrying a Budget.
    pub fn times(&self) -> Option<f64> {
        (self.n >= SCORED_MIN_RUNS).then(|| ((self.p90 * self.set_with).max(1.0) * 100.0).round() / 100.0)
    }

    pub fn line(&self) -> String {
        let times = self.times().map_or_else(|| format!("multiplier {BUDGET_TIMES}x (constant: n under {SCORED_MIN_RUNS})"), |times| format!("multiplier {times}x (from p90)"));
        format!("budget {}: n {}, over Budget {:.0}%, actual/E p50 {:.2}x p90 {:.2}x, {times}", self.category, self.n, self.over * 100.0, self.p50, self.p90)
    }
}

/// The week's line on how many runs carry a stored Budget, across every category.
pub fn summary(window: &str, scores: &[Score]) -> String {
    let n: usize = scores.iter().map(|score| score.n).sum();
    let ready = scores.iter().filter(|score| score.times().is_some()).count();
    format!("budgets {window}: n {n} runs carrying a stored Budget; {ready} of {} categories at n >= {SCORED_MIN_RUNS}", scores.len())
}

/// Each scored category's multiplier, for the budget cells.
pub fn multipliers(scores: &[Score]) -> BTreeMap<String, (f64, String)> {
    scores
        .iter()
        .filter_map(|score| Some((score.category.clone(), (score.times()?, format!("p90 {:.2}x of actual/E over n={} runs carrying a Budget, set at {:.2}x", score.p90, score.n, score.set_with)))))
        .collect()
}
