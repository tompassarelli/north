//! The arms dispatch runs live: each category's arms with the ledger boxes behind them and the share
//! of that category's boxes each takes. The shares are dispatch's own (`dispatch::shares`): equal at
//! the start, then Thompson sampling on cost per kept box with a floor per arm until `settle::settled`
//! decides the category, and every launch draws its arm from them.
use std::collections::BTreeMap;

use north_dispatch::dispatch::shares::{self, Shares};
use north_dispatch::ledger::closed::ClosedReport;
use north_dispatch::router::settle;

#[derive(Debug, PartialEq)]
pub struct Arm {
    pub category: String,
    pub arm: String,
    pub tier: String,
    pub family: &'static str,
    pub share: f64,
    pub settled: bool,
    pub boxes: usize,
    pub closed: usize,
    pub kept: usize,
    pub verified: usize,
    pub usd: f64,
}

impl Arm {
    pub fn usd_per_kept(&self) -> Option<f64> {
        (self.kept > 0).then(|| self.usd / self.kept as f64)
    }
}

/// An arm the window's boxes name: the tier its first box ran on and its checker-verified kept boxes.
#[derive(Clone, Copy)]
struct Named<'a> {
    tier: &'a str,
    verified: usize,
}

/// Every experiment the window's boxes carry, each arm with its share of the experiment's boxes in its
/// category: the arms dispatch draws between when it runs the experiment, else the arms the boxes name.
pub fn live(report: &ClosedReport) -> Vec<Arm> {
    let mut experiments: BTreeMap<(&str, &'static str), BTreeMap<&str, Named>> = BTreeMap::new();
    for armed in &report.arms {
        let arm = experiments.entry((armed.category.as_str(), settle::family(&armed.arm))).or_default().entry(armed.arm.as_str()).or_insert(Named { tier: armed.tier.as_str(), verified: 0 });
        arm.verified += usize::from(armed.closed && !armed.reworked && armed.verified);
    }
    let mut live = Vec::new();
    for ((category, family), named) in experiments {
        let outcomes = shares::arm_outcomes(report, category);
        let drawn = shares::experiment_arms(category, family);
        let arms = if drawn.is_empty() { named.keys().map(|arm| (*arm).to_owned()).collect() } else { drawn };
        let experiment = Shares::of(category, family, &arms, &outcomes);
        for (name, share) in experiment.arms {
            let boxes = outcomes.get(&name).map_or(&[][..], Vec::as_slice);
            let Named { tier, verified } = named.get(name.as_str()).copied().unwrap_or(Named { tier: "", verified: 0 });
            live.push(Arm {
                category: category.to_owned(),
                tier: tier.to_owned(),
                family,
                share,
                settled: experiment.settled,
                boxes: boxes.len(),
                closed: settle::closed(boxes),
                kept: boxes.iter().filter(|outcome| outcome.kept).count(),
                verified,
                usd: boxes.iter().map(|outcome| outcome.usd).sum(),
                arm: name,
            });
        }
    }
    live
}
