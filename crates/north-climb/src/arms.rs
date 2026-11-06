//! The arms dispatch runs live: each category's arms with the ledger boxes behind them and the share
//! of that category's boxes each should take. Shares start equal and move by Thompson sampling on cost
//! per kept box (`settle::shares`), each arm keeping a floor until `settle::settled` decides the category.
use std::collections::BTreeMap;

use north_dispatch::router::settle::{self, Outcome};
use north_dispatch::ledger::closed::ClosedReport;

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

struct Ledgered<'a> {
    tier: &'a str,
    outcomes: Vec<Outcome>,
    verified: usize,
}

/// Every arm the window's boxes carry, with the share of its experiment's boxes in its category it should take.
pub fn live(report: &ClosedReport) -> Vec<Arm> {
    let mut categories: BTreeMap<(&str, &str), BTreeMap<&str, Ledgered>> = BTreeMap::new();
    for armed in &report.arms {
        let arm = categories.entry((armed.category.as_str(), settle::family(&armed.arm))).or_default().entry(armed.arm.as_str()).or_insert(Ledgered { tier: armed.tier.as_str(), outcomes: Vec::new(), verified: 0 });
        arm.outcomes.push(Outcome { usd: armed.usd, closed: armed.closed, kept: armed.closed && !armed.reworked });
        arm.verified += usize::from(armed.closed && !armed.reworked && armed.verified);
    }
    let mut live = Vec::new();
    for ((category, family), arms) in categories {
        let boxes: Vec<&[Outcome]> = arms.values().map(|arm| arm.outcomes.as_slice()).collect();
        let (shares, settled) = settle::shares(&boxes, &format!("{category} {family}"));
        for ((name, arm), share) in arms.into_iter().zip(shares) {
            let count = |keep: fn(&Outcome) -> bool| arm.outcomes.iter().filter(|outcome| keep(outcome)).count();
            live.push(Arm {
                category: category.to_owned(),
                arm: name.to_owned(),
                tier: arm.tier.to_owned(),
                family,
                share,
                settled,
                boxes: arm.outcomes.len(),
                closed: count(|outcome| outcome.closed),
                kept: count(|outcome| outcome.kept),
                verified: arm.verified,
                usd: arm.outcomes.iter().map(|outcome| outcome.usd).sum(),
            });
        }
    }
    live
}

