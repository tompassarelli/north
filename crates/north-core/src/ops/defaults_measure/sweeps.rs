use std::collections::BTreeMap;

use rusqlite::Connection;

use super::registry::Registry;
use super::row::Row;
use crate::platform::twister::Twister;
use crate::foundation::clock::Timestamp;
use crate::threads::calibration::{
    Knobs, Raw, Run, box_chains, box_duration, box_rows, calibration_runs, escalated, moment, quantile, raw_runs, recommend, recommend_usd, tier_key,
    walk_forward_eta,
};

pub(super) const TIER_GRIDS: [(&str, &[f64]); 3] = [
    ("success_threshold", &[60.0, 70.0, 75.0, 80.0, 85.0, 90.0]),
    ("min_runs", &[3.0, 5.0, 8.0, 10.0]),
    ("incumbent_margin", &[0.0, 5.0, 10.0, 15.0, 20.0, 30.0]),
];
pub(super) const ETA_MIN_SAMPLES_GRID: [f64; 5] = [1.0, 2.0, 3.0, 5.0, 8.0];
pub(super) const ETA_COVERAGE_GRID: [f64; 6] = [0.5, 0.6, 0.7, 0.75, 0.8, 0.9];
const THIN_GRID: [usize; 9] = [3, 5, 10, 20, 30, 50, 75, 100, 150];
const BOX_GAP_GRID: [f64; 7] = [10.0, 20.0, 30.0, 45.0, 60.0, 90.0, 120.0];
const BOX_N_GRID: [usize; 7] = [5, 20, 50, 100, 150, 200, 300];
const BOOTSTRAP_REPS: usize = 400;
const BOOTSTRAP_SEED: u32 = 35;

pub(super) fn knobs(defaults: &Registry) -> Knobs {
    let min_runs = defaults.number("min_runs") as usize;
    Knobs {
        success_threshold: defaults.number("success_threshold"),
        min_runs,
        incumbent_margin: defaults.number("incumbent_margin") as i64,
        usd_min: min_runs,
        eta_min_samples: defaults.number("eta_min_samples") as usize,
        eta_coverage: defaults.number("eta_coverage"),
        box_chain_gap_min: defaults.number("box_chain_gap_min"),
        box_calibration_n: defaults.number("box_calibration_n") as usize,
        calibration_thin_n: defaults.number("calibration_thin_n") as usize,
        calibration_stale_days: defaults.number("calibration_stale_days"),
    }
}

fn grid_value(value: f64) -> serde_json::Value {
    if value.fract() == 0.0 { serde_json::Value::from(value as i64) } else { serde_json::Value::from(value) }
}

struct FirstRuns {
    rows: Vec<Raw>,
    followed: BTreeMap<String, String>,
    escalated: std::collections::BTreeSet<String>,
}

fn first_runs(db: &Connection) -> rusqlite::Result<FirstRuns> {
    let mut rows: Vec<Raw> = raw_runs(db, "follows IS NULL AND outcome != 'split' ORDER BY ended", &[])?.into_iter().filter(|raw| !raw.unjudged()).collect();
    rows.sort_by(|a, b| a.ended.cmp(&b.ended));
    let mut followed: BTreeMap<String, String> = BTreeMap::new();
    for raw in raw_runs(db, "follows IS NOT NULL", &[])? {
        let follows = raw.follows.clone().unwrap_or_default();
        let earliest = followed.get(&follows).map_or(raw.ended.clone(), |known| known.clone().min(raw.ended.clone()));
        followed.insert(follows, earliest);
    }
    Ok(FirstRuns { rows, followed, escalated: escalated(db)? })
}

pub(super) fn first_run_count(db: &Connection) -> rusqlite::Result<usize> {
    Ok(first_runs(db)?.rows.len())
}

pub(super) fn least_explored_tier_runs(db: &Connection, explore_runs: usize) -> rusqlite::Result<usize> {
    let mut runs: BTreeMap<String, usize> = BTreeMap::new();
    for tier in db.prepare("SELECT tier FROM runs WHERE role = 'worker'")?.query_map([], |row| row.get::<_, Option<String>>(0))? {
        *runs.entry(tier_key(tier?.as_deref().unwrap_or_default())).or_default() += 1;
    }
    Ok(runs.into_values().filter(|n| *n >= explore_runs).min().unwrap_or(0))
}

struct Decision<'a> {
    run: &'a Raw,
    pick: Option<String>,
    ok: bool,
}

fn tier_picks<'a>(knobs: &Knobs, first: &'a FirstRuns, since: &str) -> Vec<Decision<'a>> {
    let mut by_category: BTreeMap<&str, Vec<&Raw>> = BTreeMap::new();
    for raw in &first.rows {
        by_category.entry(raw.cells[0].as_str()).or_default().push(raw);
    }
    let mut held: BTreeMap<String, String> = BTreeMap::new();
    let mut out = Vec::new();
    for run in first.rows.iter().filter(|raw| raw.ended.as_str() >= since && raw.cells[0] != "unknown") {
        let at = run.started.as_deref().filter(|started| !started.is_empty()).unwrap_or(&run.ended);
        let asked: [Option<String>; 5] = std::array::from_fn(|index| Some(run.cells[index].clone()).filter(|value| value != "-"));
        let history: Vec<Run> = by_category[run.cells[0].as_str()]
            .iter()
            .filter(|earlier| earlier.ended.as_str() < at)
            .map(|earlier| earlier.recommend_run(earlier.succeeded(first.followed.get(&earlier.agent).is_some_and(|follow| follow.as_str() < at))))
            .collect();
        let pick = recommend(knobs, &history, &asked, &held, true);
        match &pick.tier {
            Some(tier) => held.insert(pick.cell.clone(), tier.clone()),
            None => held.remove(&pick.cell),
        };
        out.push(Decision { run, pick: pick.tier, ok: run.succeeded(first.escalated.contains(&run.agent)) });
    }
    out
}

pub(super) fn picks(db: &Connection, defaults: &Registry) -> rusqlite::Result<Vec<Row>> {
    let first = first_runs(db)?;
    Ok(tier_picks(&knobs(defaults), &first, "")
        .into_iter()
        .map(|decision| {
            Row::new().with("agent", decision.run.agent.as_str()).with("pick", decision.pick).with("ok", decision.ok).with("cost_usd", decision.run.cost_usd)
        })
        .collect())
}

fn score_picks(decisions: &[Decision]) -> Row {
    let hit: Vec<&Decision> = decisions.iter().filter(|decision| Some(tier_key(&decision.run.tier)) == decision.pick).collect();
    let (n, wins) = (hit.len(), hit.iter().filter(|decision| decision.ok).count());
    let priced: Vec<(bool, f64)> = hit.iter().filter_map(|decision| decision.run.cost_usd.map(|usd| (decision.ok, usd))).collect();
    let priced_wins = priced.iter().filter(|(ok, _)| *ok).count();
    let rate = (n > 0).then(|| wins as f64 / n as f64);
    let (lo, hi) = rate.map_or((0.into(), 0.into()), |rate| {
        let (lo, hi) = crate::ops::report::wilson(rate, n);
        (serde_json::Value::from(lo), serde_json::Value::from(hi))
    });
    Row::new()
        .with("decisions", decisions.len())
        .with("n", n)
        .with("ok", wins)
        .with("rate", rate)
        .with("lo", lo)
        .with("hi", hi)
        .with("usd_per_success", (priced_wins > 0).then(|| priced.iter().map(|(_, usd)| usd).sum::<f64>() / priced_wins as f64))
        .with("priced", priced.len())
}

pub(super) fn sweep_tier(db: &Connection, defaults: &Registry, since: &str, name: &str, values: Option<&[f64]>) -> rusqlite::Result<Vec<Row>> {
    let grid = TIER_GRIDS.iter().find(|(grid, _)| *grid == name).map_or(&[][..], |(_, grid)| grid);
    let first = first_runs(db)?;
    let base = knobs(defaults);
    Ok(values
        .unwrap_or(grid)
        .iter()
        .map(|value| {
            let mut patched = base.clone();
            match name {
                "success_threshold" => patched.success_threshold = *value,
                "min_runs" => (patched.min_runs, patched.usd_min) = (*value as usize, *value as usize),
                _ => patched.incumbent_margin = *value as i64,
            }
            let scored = score_picks(&tier_picks(&patched, &first, since));
            Row::new().with("value", grid_value(*value)).extended(scored)
        })
        .collect())
}

fn eta_pairs(knobs: &Knobs, runs: &[Run], floor: usize, since: &str) -> Vec<(Run, f64)> {
    let patched = Knobs { eta_min_samples: floor, ..knobs.clone() };
    walk_forward_eta(&patched, runs, since, "9999").into_iter().map(|(run, eta)| (run.clone(), eta)).collect()
}

pub(super) fn sweep_eta_min_samples(db: &Connection, defaults: &Registry, since: &str, values: Option<&[f64]>) -> rusqlite::Result<Vec<Row>> {
    let (base, runs) = (knobs(defaults), calibration_runs(db)?);
    Ok(values
        .unwrap_or(&ETA_MIN_SAMPLES_GRID)
        .iter()
        .map(|floor| {
            let pairs: Vec<(f64, f64)> =
                eta_pairs(&base, &runs, *floor as usize, since).into_iter().filter_map(|(run, eta)| run.minutes.filter(|minutes| *minutes > 0.0).map(|minutes| (minutes, eta))).collect();
            let errors: Vec<f64> = pairs.iter().map(|(minutes, eta)| (minutes / eta).ln().abs()).collect();
            let covered = (!pairs.is_empty()).then(|| pairs.iter().filter(|(minutes, eta)| minutes <= eta).count() as f64 / pairs.len() as f64);
            Row::new()
                .with("value", grid_value(*floor))
                .with("n", pairs.len())
                .with("median_abs_log_err", quantile(&errors, 0.5))
                .with("p75_abs_log_err", quantile(&errors, 0.75))
                .with("covered", covered)
        })
        .collect())
}

pub(super) fn sweep_eta_coverage(db: &Connection, defaults: &Registry, since: &str, values: Option<&[f64]>) -> rusqlite::Result<Vec<Row>> {
    let (base, runs) = (knobs(defaults), calibration_runs(db)?);
    let overdue = defaults.number("overdue_ratio");
    let mut pairs = eta_pairs(&base, &runs, base.eta_min_samples, "");
    pairs.sort_by(|a, b| a.0.ended.cmp(&b.0.ended));
    Ok(values
        .unwrap_or(&ETA_COVERAGE_GRID)
        .iter()
        .map(|coverage| {
            let (mut false_alarms, mut unflagged, mut n, mut over_quote) = (0usize, 0usize, 0usize, 0usize);
            let mut ratios: Vec<f64> = Vec::new();
            for (run, eta) in &pairs {
                let minutes = run.minutes.unwrap_or_default();
                if run.ended.as_str() >= since && ratios.len() >= base.calibration_thin_n {
                    let quote = eta * quantile(&ratios, *coverage).unwrap_or_default();
                    let flagged = minutes > overdue * quote;
                    let done = run.outcome == "done";
                    n += 1;
                    false_alarms += usize::from(flagged && done);
                    unflagged += usize::from(!done && !flagged);
                    over_quote += usize::from(minutes > quote);
                }
                ratios.push(minutes / eta);
            }
            Row::new()
                .with("value", grid_value(*coverage))
                .with("n", n)
                .with("overdue_false_alarms", false_alarms)
                .with("unfinished_unflagged", unflagged)
                .with("cost", false_alarms + unflagged)
                .with("over_quote", over_quote)
        })
        .collect())
}

fn usd_ratios(runs: &[Run], since: &str) -> Vec<f64> {
    runs.iter()
        .filter(|run| run.ended.as_str() >= since)
        .filter_map(|run| {
            let cost = run.cost_usd?;
            let earlier: Vec<&Run> = runs.iter().filter(|earlier| earlier.ended < run.ended).collect();
            let (usd, _) = recommend_usd(&earlier, run.category(), &run.tier);
            usd.filter(|usd| *usd != 0.0).map(|usd| cost / usd)
        })
        .collect()
}

fn half_width(ratios: &[f64], n: usize, twister: &mut Twister) -> Option<f64> {
    let full = quantile(ratios, 0.75)?;
    let mut draws: Vec<f64> = (0..BOOTSTRAP_REPS).filter_map(|_| quantile(&twister.choices(ratios, n), 0.75)).collect();
    draws.sort_by(f64::total_cmp);
    Some((quantile(&draws, 0.9)? - quantile(&draws, 0.1)?) / 2.0 / full)
}

pub(super) fn sweep_thin_n(db: &Connection, defaults: &Registry, since: &str) -> rusqlite::Result<Vec<Row>> {
    let (base, runs) = (knobs(defaults), calibration_runs(db)?);
    let eta: Vec<f64> = eta_pairs(&base, &runs, base.eta_min_samples, since).iter().map(|(run, eta)| run.minutes.unwrap_or_default() / eta).collect();
    let pools = [("eta_pred", eta), ("usd", usd_ratios(&runs, since))];
    let mut twister = Twister::seeded(BOOTSTRAP_SEED);
    Ok(THIN_GRID
        .iter()
        .map(|n| {
            let mut row = Row::new().with("value", *n);
            for (kind, ratios) in &pools {
                row.set(format!("{kind}_pool"), ratios.len());
                row.set(format!("{kind}_half_width_80"), half_width(ratios, *n, &mut twister));
            }
            row
        })
        .collect())
}

fn minutes_between(from: Timestamp, to: Timestamp) -> f64 {
    (to.unix_nanos() - from.unix_nanos()) as f64 / 60e9
}

fn box_item_gaps(db: &Connection) -> rusqlite::Result<Vec<f64>> {
    let mut by_item: BTreeMap<String, Vec<(Timestamp, Timestamp)>> = BTreeMap::new();
    for run in box_rows(db)?.into_iter().filter(|run| run.outcome != "split" && !run.ended.is_empty()) {
        let begin = [run.staffed.as_deref(), run.started.as_deref()].into_iter().flatten().find(|text| !text.is_empty()).unwrap_or(&run.ended);
        if let (Some(begin), Some(ended)) = (moment(begin), moment(&run.ended)) {
            by_item.entry(run.item.clone()).or_default().push((begin, ended));
        }
    }
    let mut gaps = Vec::new();
    for mut spans in by_item.into_values() {
        spans.sort();
        let mut end: Option<Timestamp> = None;
        for (begin, ended) in spans {
            if let Some(end) = end {
                gaps.push(minutes_between(end, begin));
            }
            end = Some(end.map_or(ended, |end| end.max(ended)));
        }
    }
    Ok(gaps)
}

pub(super) fn sweep_box_gap(db: &Connection, defaults: &Registry) -> rusqlite::Result<Vec<Row>> {
    let (base, gaps, rows) = (knobs(defaults), box_item_gaps(db)?, box_rows(db)?);
    let share = |within: usize| (!gaps.is_empty()).then(|| within as f64 / gaps.len() as f64);
    Ok(BOX_GAP_GRID
        .iter()
        .map(|gap| {
            let chains = box_chains(&Knobs { box_chain_gap_min: *gap, ..base.clone() }, &rows);
            let minutes: Vec<f64> = chains.iter().map(|chain| chain.minutes).collect();
            Row::new()
                .with("value", grid_value(*gap))
                .with("gaps", gaps.len())
                .with("share_within", share(gaps.iter().filter(|g| **g <= *gap).count()))
                .with("gaps_in_bin", gaps.iter().filter(|g| gap - 10.0 < **g && **g <= *gap).count())
                .with("chains", chains.len())
                .with("box_p75", quantile(&minutes, 0.75))
        })
        .collect())
}

pub(super) fn sweep_box_n(db: &Connection, defaults: &Registry, since: &str) -> rusqlite::Result<Vec<Row>> {
    let base = knobs(defaults);
    let chains = box_chains(&base, &box_rows(db)?);
    let since = moment(since);
    let ratios: Vec<f64> = chains
        .iter()
        .filter(|chain| since.is_none_or(|since| chain.end >= since))
        .filter_map(|chain| {
            let earlier: Vec<_> = chains.iter().filter(|earlier| earlier.end < chain.start).cloned().collect();
            let (predicted, n, _) = box_duration(&base, &earlier, chain.category.as_deref());
            predicted.filter(|predicted| n > 0 && *predicted != 0.0).map(|predicted| chain.minutes / predicted)
        })
        .collect();
    let mut twister = Twister::seeded(BOOTSTRAP_SEED);
    let full = quantile(&ratios, 0.75);
    Ok(BOX_N_GRID
        .iter()
        .map(|n| Row::new().with("value", *n).with("pool", ratios.len()).with("p75", full).with("half_width_80", half_width(&ratios, *n, &mut twister)))
        .collect())
}
