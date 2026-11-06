//! The arms dispatch runs live: each category's arms with the ledger boxes behind them and the share
//! of that category's boxes each takes.
use std::collections::BTreeMap;

use north_dispatch::ledger::closed::ClosedReport;

#[derive(Debug, PartialEq)]
pub struct Arm {
    pub category: String,
    pub arm: String,
    pub tier: String,
    pub share: f64,
    pub boxes: usize,
    pub closed: usize,
    pub kept: usize,
    pub usd: f64,
}

impl Arm {
    pub fn usd_per_kept(&self) -> Option<f64> {
        (self.kept > 0).then(|| self.usd / self.kept as f64)
    }
}

/// Every arm the week's boxes carry. Dispatch alternates a category's arms, so each takes an equal share of its category.
pub fn live(report: &ClosedReport) -> Vec<Arm> {
    let mut sums: BTreeMap<(&str, &str), (usize, usize, usize, f64)> = BTreeMap::new();
    let mut tiers: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for arm in &report.arms {
        tiers.entry((arm.category.as_str(), arm.arm.as_str())).or_insert(arm.tier.as_str());
        let sum = sums.entry((arm.category.as_str(), arm.arm.as_str())).or_default();
        sum.0 += 1;
        sum.1 += usize::from(arm.closed);
        sum.2 += usize::from(arm.closed && !arm.reworked);
        sum.3 += arm.usd;
    }
    let mut per_category: BTreeMap<&str, usize> = BTreeMap::new();
    for (category, _) in sums.keys() {
        *per_category.entry(category).or_default() += 1;
    }
    sums.into_iter().map(|((category, arm), (boxes, closed, kept, usd))| Arm { category: category.to_owned(), arm: arm.to_owned(), tier: tiers[&(category, arm)].to_owned(), share: 1.0 / per_category[category] as f64, boxes, closed, kept, usd }).collect()
}
