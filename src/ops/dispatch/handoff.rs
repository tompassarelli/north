use super::record::Record;
use super::tier::Tier;
use crate::fleet::handoff::{NO_HANDOFF, arm};
use std::collections::BTreeMap;

pub(crate) const ARM_RUNS: usize = 30;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Arms(BTreeMap<(String, String), usize>);

impl Arms {
    pub(crate) fn new(records: &[Record]) -> Self {
        let mut arms = Self::default();
        for record in records.iter().filter(|record| record.launched && !record.cloud) {
            if let (Some(tier), Some(handoff)) = (&record.tier, &record.handoff) {
                *arms.0.entry((tier.clone(), handoff.clone())).or_default() += 1;
            }
        }
        arms
    }

    pub(crate) fn pick(&mut self, tier: Tier) -> Option<String> {
        let runs = |arms: &Self, label: &str| arms.0.get(&(tier.name().to_owned(), label.to_owned())).copied().unwrap_or(0);
        let labels = [arm(crate::pace::limits::handoff_ctx()), NO_HANDOFF.to_owned()];
        let next = labels.iter().filter(|label| runs(self, label) < ARM_RUNS).min_by_key(|label| runs(self, label))?.clone();
        *self.0.entry((tier.name().to_owned(), next.clone())).or_default() += 1;
        Some(next)
    }
}

pub(crate) fn armed(brief: &str, handoff: &str) -> String {
    let line = format!("Arm: {handoff}");
    let mut lines: Vec<&str> = brief.lines().collect();
    let at = lines.iter().position(|line| line.starts_with("Tier: ")).map_or(0, |tier| tier + 1);
    lines.insert(at, &line);
    lines.join("\n")
}
