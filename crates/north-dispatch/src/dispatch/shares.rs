//! The shares an experiment's arms hold of a category's next boxes, read off the ledger's closed-box
//! report: what `north climb run --dry-run` prints and what every dispatched or hand launch draws from.
use super::tier::{ARMED, CONTEXT_WINDOWS, Effort, PLANNED, Tier};
use crate::ledger::closed::ClosedReport;
use crate::router::settle::{self, Outcome};
use std::collections::BTreeMap;

/// One experiment's arms in a category with each arm's share of the category's next boxes, as
/// `settle::shares` gives them from the arms' boxes: what the climber prints and what a launch draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Shares {
    pub category: String,
    pub family: &'static str,
    pub arms: Vec<(String, f64)>,
    pub settled: bool,
}

impl Shares {
    /// The shares of `arms`, each holding its boxes in `outcomes`; an arm with none starts level.
    pub fn of(category: &str, family: &'static str, arms: &[String], outcomes: &BTreeMap<String, Vec<Outcome>>) -> Self {
        let boxes: Vec<&[Outcome]> = arms.iter().map(|arm| outcomes.get(arm).map_or(&[][..], Vec::as_slice)).collect();
        let (shares, settled) = settle::shares(&boxes, &format!("{category} {family}"));
        Self { category: category.to_owned(), family, arms: arms.iter().cloned().zip(shares).collect(), settled }
    }

    /// The arm the launch seeded `seed` draws, and the words naming the share it drew from.
    pub fn draw(&self, seed: &str) -> (&str, String) {
        let shares: Vec<f64> = self.arms.iter().map(|(_, share)| *share).collect();
        let (arm, share) = &self.arms[settle::pick(&shares, seed)];
        let listed: Vec<String> = self.arms.iter().map(|(arm, share)| format!("{arm} {:.0}%", 100.0 * share)).collect();
        (arm, format!("drawn at its {:.0}% share of the {} {} arms ({}) seeded {seed}", 100.0 * share, self.category, self.family, listed.join(", ")))
    }
}

/// The arms dispatch draws between in `category`'s `family` experiment, in the order the climber lists
/// them; empty for an experiment dispatch does not draw.
pub fn experiment_arms(category: &str, family: &str) -> Vec<String> {
    let mut arms: Vec<String> = match family {
        "tier" if ARMED.contains(&category) => [Tier::Sonnet, Tier::Opus].map(|tier| tier.name().to_owned()).to_vec(),
        "effort" if PLANNED.contains(&category) => [Effort::High, Effort::Xhigh].map(|effort| effort.arm().to_owned()).to_vec(),
        "effort" => Effort::ARMS.map(|effort| effort.arm().to_owned()).to_vec(),
        "context" => CONTEXT_WINDOWS.map(crate::fleet::compaction::arm).to_vec(),
        _ => Vec::new(),
    };
    arms.sort();
    arms
}

/// Each arm's boxes in `category`, closed or not, from the closed-box report, keyed by the arm's stamp.
pub fn arm_outcomes(report: &ClosedReport, category: &str) -> BTreeMap<String, Vec<Outcome>> {
    let mut outcomes: BTreeMap<String, Vec<Outcome>> = BTreeMap::new();
    for armed in report.arms.iter().filter(|armed| armed.category == category) {
        outcomes.entry(armed.arm.clone()).or_default().push(Outcome { usd: armed.usd, closed: armed.closed, kept: armed.closed && !armed.reworked });
    }
    outcomes
}

