use super::record::Record;
use super::tier::Tier;
use crate::fleet::handoff::{NO_HANDOFF, arm};
use super::fleets::{self, PROBE};
use crate::ledger::ClosedReport;
use crate::ledger::closed::ArmBox;
use toml_edit::DocumentMut;
use std::collections::{BTreeMap, BTreeSet};

pub const ARM_RUNS: usize = 30;
const FAMILIES: [Tier; 3] = [Tier::Haiku, Tier::Sonnet, Tier::Opus];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Arms {
    runs: BTreeMap<(String, String), usize>,
    usd_per_closed: BTreeMap<(Tier, String), f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Winner {
    pub tier: Tier,
    pub arm: String,
    pub usd: f64,
    pub other: String,
    pub other_usd: f64,
}

impl Winner {
    fn recommendation(&self) -> String {
        let advice = if self.arm == NO_HANDOFF {
            "drop the worker handoff point for this tier and leave Claude Code's own compaction to it"
        } else {
            "keep the worker handoff point for this tier"
        };
        format!(
            "north#57 handoff arms, {}: {} wins at ${:.2} per closed box against {} at ${:.2} over {ARM_RUNS} runs each; recommend: {advice}",
            self.tier.name(),
            self.arm,
            self.usd,
            self.other,
            self.other_usd
        )
    }
}

impl Arms {
    pub fn new(records: &[Record]) -> Self {
        let mut arms = Self::default();
        for record in records.iter().filter(|record| record.launched && !record.cloud) {
            if let (Some(tier), Some(handoff)) = (&record.tier, &record.handoff) {
                *arms.runs.entry((tier.clone(), handoff.clone())).or_default() += 1;
            }
        }
        arms
    }

    pub fn priced(self, boxes: &[ArmBox]) -> Self {
        let mut sums: BTreeMap<(Tier, String), (f64, usize)> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for closed in boxes.iter().filter(|closed| seen.insert(closed.item.as_str())) {
            if let Some(tier) = FAMILIES.into_iter().find(|tier| tier.serves(&closed.tier)) {
                let sum = sums.entry((tier, closed.arm.clone())).or_default();
                *sum = (sum.0 + closed.usd, sum.1 + 1);
            }
        }
        Self { usd_per_closed: sums.into_iter().map(|(key, (usd, closed))| (key, usd / closed as f64)).collect(), ..self }
    }

    fn runs(&self, tier: Tier, label: &str) -> usize {
        self.runs.get(&(tier.name().to_owned(), label.to_owned())).copied().unwrap_or(0)
    }

    fn labels() -> [String; 2] {
        [arm(crate::pace::limits::handoff_arm_ctx()), NO_HANDOFF.to_owned()]
    }

    pub fn winner(&self, tier: Tier) -> Option<Winner> {
        let [handoff, none] = Self::labels();
        if self.runs(tier, &handoff) < ARM_RUNS || self.runs(tier, &none) < ARM_RUNS {
            return None;
        }
        let cost = |label: &str| self.usd_per_closed.get(&(tier, label.to_owned())).copied();
        let (with, without) = (cost(&handoff)?, cost(&none)?);
        let (arm, usd, other, other_usd) = if without < with { (none, without, handoff, with) } else { (handoff, with, none, without) };
        Some(Winner { tier, arm, usd, other, other_usd })
    }

    pub fn recommendations(&self) -> Vec<String> {
        FAMILIES.into_iter().filter_map(|tier| self.winner(tier)).map(|winner| winner.recommendation()).collect()
    }

    pub fn pick(&mut self, tier: Tier) -> Option<String> {
        let next = match self.winner(tier) {
            Some(winner) => winner.arm,
            None => Self::labels().into_iter().min_by_key(|label| (self.runs(tier, label) >= ARM_RUNS, self.runs(tier, label)))?,
        };
        *self.runs.entry((tier.name().to_owned(), next.clone())).or_default() += 1;
        Some(next)
    }
}

pub fn boxes(local: Option<&ClosedReport>, cfg: &DocumentMut, here: &str, notes: &mut Vec<String>) -> Vec<ArmBox> {
    let remote = fleets::reached(cfg, here, &["ledger", "summary", "--since", super::tier::ARM_WINDOW, "--json"], PROBE, notes);
    let remote = remote.into_iter().flat_map(|(_, listing)| serde_json::from_value::<Vec<ArmBox>>(listing["arms"].clone()).unwrap_or_default());
    local.into_iter().flat_map(|report| report.arms.iter().cloned()).chain(remote).collect()
}

pub fn armed(brief: &str, handoff: &str) -> String {
    let line = format!("Arm: {handoff}");
    let mut lines: Vec<&str> = brief.lines().collect();
    let at = lines.iter().position(|line| line.starts_with("Tier: ")).map_or(0, |tier| tier + 1);
    lines.insert(at, &line);
    lines.join("\n")
}
