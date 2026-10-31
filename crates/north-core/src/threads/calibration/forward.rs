//! Walk-forward calibration: each past run's actual against what the core would have predicted from strictly earlier runs.
use std::collections::BTreeMap;

use super::boxes::{BoxChain, box_duration, moment};
use super::recommend::{recommend_eta, recommend_usd, tier_stats};
use super::{GROUPS, Knobs, PROVIDERS, Run, provider, quantile, tier_rank};

/// A number as the calibration table keeps it: a whole success rate or a real ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Figure {
    Whole(i64),
    Real(f64),
}

impl Figure {
    pub fn real(self) -> f64 {
        match self {
            Self::Whole(whole) => whole as f64,
            Self::Real(real) => real,
        }
    }
}

/// One calibration row: the cell it covers, its sample size, the ratio quartiles and the value recommend applies.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub cell: String,
    pub n: usize,
    pub p25: Option<f64>,
    pub median: Option<f64>,
    pub p75: Option<f64>,
    pub value: Option<Figure>,
}

/// One row per provider and `all` from (tier, actual / predicted) pairs, value the scale covering `eta_coverage`.
fn ratio_rows(knobs: &Knobs, pairs: &[(String, f64)]) -> Vec<Row> {
    PROVIDERS
        .iter()
        .map(|cell| {
            let ratios: Vec<f64> = pairs.iter().filter(|(tier, _)| *cell == "all" || provider(tier) == *cell).map(|(_, ratio)| *ratio).collect();
            Row {
                cell: (*cell).to_owned(),
                n: ratios.len(),
                p25: quantile(&ratios, 0.25),
                median: quantile(&ratios, 0.5),
                p75: quantile(&ratios, 0.75),
                value: quantile(&ratios, knobs.eta_coverage).map(Figure::Real),
            }
        })
        .collect()
}

fn run_cell(run: &Run) -> String {
    GROUPS.iter().zip(&run.cells).filter(|(_, value)| *value != "-").map(|(key, value)| format!("{}: {value}", super::recommend::capitalized(key))).collect::<Vec<_>>().join(", ")
}

fn in_window(run: &Run, start: &str, end: &str) -> bool {
    start <= run.ended.as_str() && run.ended.as_str() < end
}

/// (run, uncalibrated recommend_eta from strictly earlier first-assignment runs of its tier) for each run ended in [start, end).
pub fn walk_forward_eta<'a>(knobs: &Knobs, runs: &'a [Run], start: &str, end: &str) -> Vec<(&'a Run, f64)> {
    let mut by_tier: BTreeMap<&str, Vec<&Run>> = BTreeMap::new();
    let mut firsts: Vec<&Run> = runs.iter().filter(|run| run.first).collect();
    firsts.sort_by(|x, y| x.ended.cmp(&y.ended));
    for run in firsts {
        by_tier.entry(&run.tier).or_default().push(run);
    }
    runs.iter()
        .filter(|run| run.first && run.minutes.is_some() && in_window(run, start, end))
        .filter_map(|run| {
            let tier_runs = by_tier.get(run.tier.as_str())?;
            let earlier = &tier_runs[..tier_runs.partition_point(|earlier| earlier.ended < run.ended)];
            let (eta, n, _) = recommend_eta(knobs, earlier, &run_cell(run), &run.tier);
            eta.filter(|eta| n > 0 && *eta != 0.0).map(|eta| (run, eta))
        })
        .collect()
}

pub fn calibrate_eta(knobs: &Knobs, runs: &[Run], start: &str, end: &str) -> Vec<Row> {
    let pairs: Vec<(String, f64)> = runs
        .iter()
        .filter(|run| in_window(run, start, end))
        .filter_map(|run| Some((run.tier.clone(), run.minutes? / run.eta_min.filter(|eta| *eta != 0.0)?)))
        .collect();
    ratio_rows(knobs, &pairs)
}

pub fn calibrate_eta_pred(knobs: &Knobs, runs: &[Run], start: &str, end: &str) -> Vec<Row> {
    let pairs: Vec<(String, f64)> = walk_forward_eta(knobs, runs, start, end).into_iter().filter_map(|(run, eta)| Some((run.tier.clone(), run.minutes? / eta))).collect();
    ratio_rows(knobs, &pairs)
}

pub fn calibrate_usd(knobs: &Knobs, runs: &[Run], start: &str, end: &str) -> Vec<Row> {
    let mut pairs = Vec::new();
    for run in runs.iter().filter(|run| in_window(run, start, end)) {
        let Some(cost) = run.cost_usd else { continue };
        let earlier: Vec<&Run> = runs.iter().filter(|earlier| earlier.ended < run.ended).collect();
        if let (Some(usd), _) = recommend_usd(&earlier, run.category(), &run.tier)
            && usd != 0.0
        {
            pairs.push((run.tier.clone(), cost / usd));
        }
    }
    ratio_rows(knobs, &pairs)
}

pub fn calibrate_tier(knobs: &Knobs, runs: &[Run], start: &str, end: &str) -> Vec<Row> {
    let mut by_category: BTreeMap<&str, Vec<&Run>> = BTreeMap::new();
    for run in runs.iter().filter(|run| run.first && !run.unjudged && in_window(run, start, end)) {
        by_category.entry(run.category()).or_default().push(run);
    }
    let blank = |cell: String, n: usize, value| Row { cell, n, p25: None, median: None, p75: None, value };
    let mut rows = Vec::new();
    for (category, group) in by_category {
        let mut stats = tier_stats(knobs, &group);
        stats.sort_by_key(|(tier, _)| tier_rank(tier));
        for (tier, stat) in stats {
            rows.push(blank(format!("{category} {tier} success"), stat.n, Some(Figure::Whole(stat.rate))));
            rows.push(blank(format!("{category} {tier} usd_per_success"), stat.usd_n, stat.usd.map(Figure::Real)));
        }
    }
    rows
}

/// Actual / predicted for each box chain landed in [start, end), predicted from chains landed before it was staffed.
pub fn calibrate_box(knobs: &Knobs, chains: &[BoxChain], start: &str, end: &str) -> Vec<Row> {
    let (Some(lo), Some(hi)) = (moment(start), moment(end)) else { return Vec::new() };
    let mut pairs = Vec::new();
    for chain in chains.iter().filter(|chain| lo <= chain.end && chain.end < hi) {
        let earlier: Vec<BoxChain> = chains.iter().filter(|earlier| earlier.end < chain.start).cloned().collect();
        if let (Some(predicted), n, _) = box_duration(knobs, &earlier, chain.category.as_deref())
            && n > 0
            && predicted != 0.0
        {
            pairs.push(("all".to_owned(), chain.minutes / predicted));
        }
    }
    ratio_rows(knobs, &pairs).into_iter().filter(|row| row.cell == "all").collect()
}
