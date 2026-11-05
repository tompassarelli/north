use super::buckets::{fit, CellFit, Fit, Target, Weights};
use super::{Era, Lane};

const LOSS_FLOOR: f64 = 1e-12;
const TIE_SCALE: f64 = 1000.0;
const DECILES: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scoring {
    LeaveOneBranchOut,
    Prequential,
}

impl Scoring {
    pub fn name(self) -> &'static str {
        match self {
            Scoring::LeaveOneBranchOut => "leave-one-branch-out",
            Scoring::Prequential => "prequential",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decile {
    pub rows: usize,
    pub mean_predicted: f64,
    pub observed_rate: f64,
    pub standard_error: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub target: Target,
    pub era: Era,
    pub scoring: Scoring,
    pub rows: usize,
    pub base_rate: f64,
    pub brier_model: f64,
    pub brier_base: f64,
    pub log_loss_model: f64,
    pub log_loss_base: f64,
    pub deciles: Vec<Decile>,
    pub cells: Vec<CellFit>,
}

struct Scored {
    predicted: f64,
    base: f64,
    outcome: bool,
}

pub fn replay(lanes: &[Lane], target: Target, era: Era, scoring: Scoring, weights: Weights) -> Report {
    let scored: Vec<Scored> = lanes
        .iter()
        .filter(|lane| lane.era == era)
        .filter_map(|lane| {
            let outcome = target.outcome(lane)?;
            let training = lanes.iter().filter(|other| other.branch != lane.branch && trained_on(scoring, lane, other));
            let model = fit(target, era, weights, training);
            Some(Scored { predicted: model.predict(lane), base: model.base, outcome })
        })
        .collect();
    let rows = scored.len();
    let mean = |score: &dyn Fn(&Scored) -> f64| if rows == 0 { 0.0 } else { scored.iter().map(score).sum::<f64>() / rows as f64 };
    let observed = |scored: &Scored| f64::from(u8::from(scored.outcome));
    let all = fit(target, era, weights, lanes.iter());
    Report {
        target,
        era,
        scoring,
        rows,
        base_rate: mean(&observed),
        brier_model: mean(&|s| (s.predicted - observed(s)).powi(2)),
        brier_base: mean(&|s| (s.base - observed(s)).powi(2)),
        log_loss_model: mean(&|s| log_loss(s.predicted, s.outcome)),
        log_loss_base: mean(&|s| log_loss(s.base, s.outcome)),
        deciles: deciles(&scored),
        cells: all.cells,
    }
}

pub fn fit_live(lanes: &[Lane], target: Target, era: Era, weights: Weights) -> Fit {
    fit(target, era, weights, lanes.iter())
}

fn trained_on(scoring: Scoring, target: &Lane, other: &Lane) -> bool {
    match scoring {
        Scoring::LeaveOneBranchOut => true,
        Scoring::Prequential => other.decided <= target.queued,
    }
}

fn log_loss(predicted: f64, outcome: bool) -> f64 {
    let p = predicted.clamp(LOSS_FLOOR, 1.0 - LOSS_FLOOR);
    -(if outcome { p.ln() } else { (1.0 - p).ln() })
}

fn tie_key(predicted: f64) -> i64 {
    (predicted * TIE_SCALE).round() as i64
}

fn deciles(scored: &[Scored]) -> Vec<Decile> {
    let mut order: Vec<&Scored> = scored.iter().collect();
    order.sort_by(|a, b| a.predicted.total_cmp(&b.predicted));
    let total = order.len();
    let mut groups: Vec<&[&Scored]> = Vec::new();
    let mut start = 0;
    for group in 1..=DECILES {
        let target = (total * group).div_ceil(DECILES);
        let mut end = target.max(start);
        while end > start && end < total && tie_key(order[end].predicted) == tie_key(order[end - 1].predicted) {
            end += 1;
        }
        if end > start {
            groups.push(&order[start..end]);
            start = end;
        }
    }
    groups
        .into_iter()
        .map(|group| {
            let rows = group.len() as f64;
            let rate = group.iter().filter(|s| s.outcome).count() as f64 / rows;
            Decile { rows: group.len(), mean_predicted: group.iter().map(|s| s.predicted).sum::<f64>() / rows, observed_rate: rate, standard_error: (rate * (1.0 - rate) / rows).sqrt() }
        })
        .collect()
}
