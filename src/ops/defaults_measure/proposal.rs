use rusqlite::Connection;
use serde_json::Value;

use super::Context;
use super::row::Row;
use super::sweeps::{self, TIER_GRIDS};

const W5: [&str; 5] = ["eta_min_samples", "eta_coverage", "calibration_thin_n", "box_chain_gap_min", "box_calibration_n"];
const TIER_REASON: &str = "production replay required; descriptive threads.recommend only; untimestamped costs unavailable; mismatched tiers unknown";

struct Scored {
    scores: Vec<Row>,
    objective: &'static str,
    direction: &'static str,
    reason: &'static str,
}

pub(super) fn rows(db: &Connection, context: &Context, kind: Option<&str>, train_until: &str) -> Result<Vec<Row>, String> {
    let mut measured: Vec<_> = context.registry.entries().iter().filter(|entry| entry.source.starts_with("measured:") && kind.is_none_or(|kind| kind == entry.name)).collect();
    measured.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = Vec::new();
    for entry in measured {
        let name = entry.name.as_str();
        let current = &entry.value;
        let report = if TIER_GRIDS.iter().any(|(grid, _)| *grid == name) {
            "W1"
        } else if W5.contains(&name) {
            "W5"
        } else {
            "unavailable"
        };
        let mut row = Row::new()
            .with("report", report)
            .with("default", name)
            .with("current", current.clone())
            .with("proposed", current.clone())
            .with("n", 0)
            .with("score_current", Value::Null)
            .with("score_proposed", Value::Null)
            .with("train_until", train_until)
            .with("since", context.since.as_str())
            .with("objective", Value::Null)
            .with("direction", Value::Null)
            .with("eligible", false)
            .with("reason", "unsupported: no implemented W1/W5 scorer");
        match score(db, context, name, current).map_err(|error| format!("{name}: {error}"))? {
            Some(scored) => choose(&mut row, name, current, &scored),
            None if W5.contains(&name) => {
                let untimestamped = if name == "calibration_thin_n" { "; untimestamped USD evidence unavailable" } else { "" };
                row.set("reason", format!("unsupported: no meaningful scalar policy scorer{untimestamped}"));
            }
            None => {}
        }
        out.push(row);
    }
    Ok(out)
}

fn candidates(current: &Value, grid: &[f64]) -> Vec<f64> {
    let mut values: Vec<f64> = grid.iter().copied().chain(current.as_f64()).collect();
    values.sort_by(f64::total_cmp);
    values.dedup();
    values
}

fn score(db: &Connection, context: &Context, name: &str, current: &Value) -> rusqlite::Result<Option<Scored>> {
    let (defaults, since) = (&context.registry, context.since.as_str());
    if let Some((_, grid)) = TIER_GRIDS.iter().find(|(grid, _)| *grid == name) {
        let scores = sweeps::sweep_tier(db, defaults, since, name, Some(&candidates(current, grid)))?;
        return Ok(Some(Scored { scores, objective: "rate", direction: "maximize", reason: TIER_REASON }));
    }
    Ok(match name {
        "eta_min_samples" => Some(Scored {
            scores: sweeps::sweep_eta_min_samples(db, defaults, since, Some(&candidates(current, &sweeps::ETA_MIN_SAMPLES_GRID)))?,
            objective: "median_abs_log_err",
            direction: "minimize",
            reason: "held-out ETA judgment required",
        }),
        "eta_coverage" => Some(Scored {
            scores: sweeps::sweep_eta_coverage(db, defaults, since, Some(&candidates(current, &sweeps::ETA_COVERAGE_GRID)))?,
            objective: "cost",
            direction: "minimize",
            reason: "proposal only: overdue false alarms plus unfinished unflagged",
        }),
        _ => None,
    })
}

fn choose(row: &mut Row, name: &str, current: &Value, scored: &Scored) {
    let objective = scored.objective;
    row.set("objective", objective);
    row.set("direction", scored.direction);
    let same = |score: &Row| score.get("value").as_f64() == current.as_f64();
    let usable = |score: &&Row| score.get("n").as_u64().is_some_and(|n| n > 0) && score.get(objective).as_f64().is_some_and(f64::is_finite);
    let Some(baseline) = scored.scores.iter().find(|score| same(score)) else { return };
    row.set("n", baseline.get("n").clone());
    let usable_scores: Vec<&Row> = scored.scores.iter().filter(usable).collect();
    if !usable(&baseline) || usable_scores.len() != scored.scores.len() {
        row.set("reason", format!("insufficient evidence for every candidate comparison; {}", scored.reason));
        return;
    }
    let sign = if scored.direction == "maximize" { -1.0 } else { 1.0 };
    let key = |score: &Row| (score.get(objective).as_f64().unwrap_or_default() * sign, !same(score), score.get("value").as_f64().unwrap_or_default());
    let best = usable_scores
        .into_iter()
        .min_by(|a, b| {
            let (a, b) = (key(a), key(b));
            a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2))
        })
        .expect("the baseline is usable");
    row.set("proposed", best.get("value").clone());
    row.set("n", best.get("n").clone());
    row.set("score_current", baseline.get(objective).clone());
    row.set("score_proposed", best.get(objective).clone());
    row.set("eligible", name == "eta_min_samples");
    row.set("reason", scored.reason);
}
