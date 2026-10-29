//! Which tier a brief's cell should run on, and the ETA and $ that go with it.
use std::collections::BTreeMap;

use super::{BACKOFF, GROUPS, Knobs, Run, quantile, tier_key, tier_rank};
use crate::threads::runs::brief::Fields;

/// What the runs of one tier say: success rate in whole percent, run count, successes, priced runs and $ per success.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TierStat {
    pub rate: i64,
    pub n: usize,
    pub ok: usize,
    pub usd_n: usize,
    pub usd: Option<f64>,
}

/// A recommendation: the cell it was measured on, that cell's run count, the tier (None for insufficient evidence),
/// every tier's stats in first-seen order, and the rule that chose.
#[derive(Clone, Debug)]
pub(crate) struct Pick {
    pub cell: String,
    pub n: usize,
    pub tier: Option<String>,
    pub stats: Vec<(String, TierStat)>,
    pub rule: String,
}

pub(crate) fn tier_stats(knobs: &Knobs, runs: &[&Run]) -> Vec<(String, TierStat)> {
    let mut by: Vec<(String, Vec<&Run>)> = Vec::new();
    for run in runs {
        let key = tier_key(&run.tier);
        match by.iter_mut().find(|(tier, _)| *tier == key) {
            Some((_, group)) => group.push(run),
            None => by.push((key, vec![run])),
        }
    }
    by.into_iter()
        .map(|(tier, group)| {
            let priced: Vec<&&Run> = group.iter().filter(|run| run.cost_usd.is_some()).collect();
            let wins = priced.iter().filter(|run| run.ok).count();
            let ok = group.iter().filter(|run| run.ok).count();
            let spent = compensated_sum(priced.iter().filter_map(|run| run.cost_usd));
            let usd = (priced.len() >= knobs.usd_min && wins > 0).then(|| spent / wins as f64);
            (tier, TierStat { rate: (ok * 100 / group.len()) as i64, n: group.len(), ok, usd_n: priced.len(), usd })
        })
        .collect()
}

fn keys_of(asked: &[Option<String>; 5]) -> Vec<usize> {
    let mut keys = vec![0];
    keys.extend(BACKOFF.iter().rev().filter_map(|facet| GROUPS.iter().position(|group| group == facet).filter(|index| asked[*index].is_some())));
    keys
}

fn cell_name(keys: &[usize], value: impl Fn(usize) -> String) -> String {
    keys.iter().map(|index| format!("{}: {}", capitalized(GROUPS[*index]), value(*index))).collect::<Vec<_>>().join(", ")
}

pub(super) fn capitalized(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

/// The finest cell holding `min_runs` runs of some tier, backing off along BACKOFF; in it the cheapest passing tier by
/// measured $ per success, or by tier rank when a passing tier has under `usd_min` priced runs or `priced` is false,
/// kept at the incumbent unless a dearer challenger beats it by `incumbent_margin` points. With no passing tier, the
/// lowest measured cost among sampled tiers, or no tier for insufficient evidence.
pub(crate) fn recommend(knobs: &Knobs, runs: &[Run], asked: &[Option<String>; 5], incumbent: &BTreeMap<String, String>, priced: bool) -> Pick {
    let mut keys = keys_of(asked);
    let (members, stats) = loop {
        let members: Vec<&Run> = runs.iter().filter(|run| keys.iter().all(|index| Some(&run.cells[*index]) == asked[*index].as_ref())).collect();
        let stats = tier_stats(knobs, &members);
        if stats.iter().any(|(_, stat)| stat.n >= knobs.min_runs) || keys.len() == 1 {
            break (members, stats);
        }
        keys.pop();
    };
    let name = cell_name(&keys, |index| asked[index].clone().unwrap_or_default());
    let sampled: Vec<&(String, TierStat)> = stats.iter().filter(|(_, stat)| stat.n >= knobs.min_runs).collect();
    let stat_of = |tier: &str| sampled.iter().find(|(key, _)| key == tier).map(|(_, stat)| stat);
    let passing: Vec<&str> = sampled.iter().filter(|(_, stat)| stat.rate as f64 >= knobs.success_threshold).map(|(tier, _)| tier.as_str()).collect();
    let mut thin: Vec<&str> = passing.iter().copied().filter(|tier| stat_of(tier).is_some_and(|stat| stat.usd.is_none())).collect();
    thin.sort_by_key(|tier| tier_rank(tier));
    let finish = |tier: Option<String>, rule: String| Pick { cell: name.clone(), n: members.len(), tier, stats: stats.clone(), rule };
    if passing.is_empty() {
        let best = sampled.iter().filter(|(_, stat)| stat.usd.is_some()).min_by(|(x, a), (y, b)| a.usd.partial_cmp(&b.usd).expect("measured $ is a number").then(tier_rank(x).cmp(&tier_rank(y))));
        let rule = if best.is_some() {
            "$ per success (no tier passes; lowest measured cost among sampled tiers)"
        } else {
            "insufficient evidence (no tier passes; no sampled tier has measured $ per success)"
        };
        return finish(best.map(|(tier, _)| tier.clone()), rule.to_owned());
    }
    let rule = if passing.len() == 1 {
        "rank (one tier passes)".to_owned()
    } else if !priced {
        "rank".to_owned()
    } else if !thin.is_empty() {
        let named: Vec<String> = thin.iter().map(|tier| format!("{tier} n={}", stat_of(tier).map_or(0, |stat| stat.usd_n))).collect();
        format!("rank (uncalibrated: thin $ {} < {})", named.join(", "), knobs.usd_min)
    } else {
        "$ per success".to_owned()
    };
    let by_usd = rule == "$ per success";
    let usd_of = |tier: &str| stat_of(tier).and_then(|stat| stat.usd).unwrap_or(f64::INFINITY);
    let mut best = passing
        .iter()
        .copied()
        .min_by(|x, y| if by_usd { usd_of(x).total_cmp(&usd_of(y)).then(tier_rank(x).cmp(&tier_rank(y))) } else { tier_rank(x).cmp(&tier_rank(y)) })
        .expect("a passing tier")
        .to_owned();
    let held = incumbent.get(&name).filter(|held| !held.is_empty()).map(|held| tier_key(held));
    if let Some(held) = held.filter(|held| *held != best && stat_of(held).is_some() && stat_of(&best).is_some()) {
        let dearer = if by_usd && passing.contains(&best.as_str()) && passing.contains(&held.as_str()) { usd_of(&best) > usd_of(&held) } else { tier_rank(&best) > tier_rank(&held) };
        let rate = |tier: &str| stat_of(tier).map_or(0, |stat| stat.rate);
        if dearer && rate(&best) < rate(&held) + knobs.incumbent_margin {
            best = held;
        }
    }
    finish(Some(best), rule)
}

/// p75 minutes of the tier's runs in the finest cell of `cell` (a brief's facet text) with `eta_min_samples` runs,
/// backing off along BACKOFF; (p75, n, cell name).
pub(crate) fn recommend_eta(knobs: &Knobs, runs: &[&Run], cell: &str, tier: &str) -> (Option<f64>, usize, String) {
    let fields = Fields::read(cell);
    let wanted = tier_key(tier);
    let asked = asked_of(&fields);
    let mut keys = keys_of(&asked);
    loop {
        let samples: Vec<f64> = runs
            .iter()
            .filter(|run| tier_key(&run.tier) == wanted && keys.iter().all(|index| Some(&run.cells[*index]) == asked[*index].as_ref()))
            .filter_map(|run| run.minutes)
            .collect();
        if samples.len() >= knobs.eta_min_samples || keys.len() == 1 {
            let name = cell_name(&keys, |index| asked[index].clone().unwrap_or_default());
            return (quantile(&samples, 0.75), samples.len(), name);
        }
        keys.pop();
    }
}

/// The brief fields of a cell text in GROUPS order, None for a field it does not name.
pub(crate) fn asked_of(fields: &Fields) -> [Option<String>; 5] {
    [fields.category.clone(), fields.facets[0].clone(), fields.facets[1].clone(), fields.facets[2].clone(), fields.facets[3].clone()]
}

/// Median $ per done run in the (category, tier) cell, backing off to the tier; None below 3 samples.
pub(crate) fn recommend_usd(runs: &[&Run], category: &str, tier: &str) -> (Option<f64>, usize) {
    let in_cell = |run: &&Run| run.tier == tier && run.category() == category;
    let in_tier = |run: &&Run| run.tier == tier;
    for keep in [&in_cell as &dyn Fn(&&Run) -> bool, &in_tier] {
        let costs: Vec<f64> = runs.iter().filter(|run| keep(run) && run.outcome == "done").filter_map(|run| run.cost_usd).collect();
        if costs.len() >= 3 {
            return (quantile(&costs, 0.5), costs.len());
        }
    }
    (None, 0)
}

/// Python 3.12's `sum` of floats: Neumaier compensation, so a $ per success equals the one bin/threads printed.
fn compensated_sum(values: impl Iterator<Item = f64>) -> f64 {
    let (mut total, mut carry) = (0.0_f64, 0.0_f64);
    for value in values {
        let next = total + value;
        carry += if total.abs() >= value.abs() { (total - next) + value } else { (value - next) + total };
        total = next;
    }
    if carry != 0.0 && carry.is_finite() { total + carry } else { total }
}
