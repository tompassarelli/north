use super::{Class, Era, Lane};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Conflict,
    Pass,
}

const CONFLICT_CELLS: [&str; 6] = ["shared 0, first", "shared 0, resubmit", "shared 1-2, first", "shared 1-2, resubmit", "shared 3+, first", "shared 3+, resubmit"];
const PASS_CELLS: [&str; 5] = ["docs-only", "other paths, clean history", "other paths, refused history", "lander paths, clean history", "lander paths, refused history"];

impl Target {
    pub const ALL: [Target; 2] = [Target::Conflict, Target::Pass];

    pub fn name(self) -> &'static str {
        match self {
            Target::Conflict => "conflict",
            Target::Pass => "pass",
        }
    }

    pub fn cell_labels(self) -> &'static [&'static str] {
        match self {
            Target::Conflict => &CONFLICT_CELLS,
            Target::Pass => &PASS_CELLS,
        }
    }

    pub fn cell(self, lane: &Lane) -> usize {
        match self {
            Target::Conflict => {
                let shared = match lane.shared_files {
                    0 => 0,
                    1 | 2 => 1,
                    _ => 2,
                };
                shared * 2 + usize::from(lane.resubmit)
            }
            Target::Pass if lane.docs_only => 0,
            Target::Pass => 1 + usize::from(lane.lander_paths) * 2 + usize::from(lane.history_refused),
        }
    }

    pub fn outcome(self, lane: &Lane) -> Option<bool> {
        match (self, lane.class) {
            (Target::Conflict, Class::Replaced) => None,
            (Target::Conflict, class) => Some(class == Class::Conflict),
            (Target::Pass, Class::Landed) => Some(true),
            (Target::Pass, Class::Failed) => Some(false),
            (Target::Pass, _) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    pub other_era: f64,
    pub pseudo_rows: f64,
}

impl Default for Weights {
    fn default() -> Weights {
        Weights { other_era: 0.25, pseudo_rows: 2.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellFit {
    pub label: &'static str,
    pub weight: f64,
    pub events: f64,
    pub alpha: f64,
    pub beta: f64,
    pub posterior: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub target: Target,
    pub live: Era,
    pub weight: f64,
    pub base: f64,
    pub cells: Vec<CellFit>,
}

impl Fit {
    pub fn predict(&self, lane: &Lane) -> f64 {
        self.cells[self.target.cell(lane)].posterior
    }
}

pub fn fit<'a>(target: Target, live: Era, weights: Weights, lanes: impl IntoIterator<Item = &'a Lane>) -> Fit {
    let labels = target.cell_labels();
    let mut cell_weight = vec![0.0; labels.len()];
    let mut cell_events = vec![0.0; labels.len()];
    let mut weight = 0.0;
    let mut events = 0.0;
    for lane in lanes {
        let Some(outcome) = target.outcome(lane) else { continue };
        let w = if lane.era == live { 1.0 } else { weights.other_era };
        let hit = if outcome { w } else { 0.0 };
        let cell = target.cell(lane);
        cell_weight[cell] += w;
        cell_events[cell] += hit;
        weight += w;
        events += hit;
    }
    let base = (events + 1.0) / (weight + 2.0);
    let cells = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let alpha = cell_events[index] + weights.pseudo_rows * base;
            let beta = cell_weight[index] - cell_events[index] + weights.pseudo_rows * (1.0 - base);
            CellFit { label, weight: cell_weight[index], events: cell_events[index], alpha, beta, posterior: alpha / (alpha + beta) }
        })
        .collect();
    Fit { target, live, weight, base, cells }
}
