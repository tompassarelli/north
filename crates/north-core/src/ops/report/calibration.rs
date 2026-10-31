//! The calibration rows north worker-ledger records, shown as the newest batch of each kind, and the
//! goal ETAs the planner predicted against what finished.
use std::collections::BTreeMap;

use serde_json::Value;

use super::{age, plain};
use crate::pace::limits::default;

pub(super) const QUERY: &str = "SELECT at, kind, cell, n, p25, median, p75, value, source FROM calibration";
const GOAL_KINDS: [&str; 2] = ["goal_eta", "goal_actual"];

pub struct Row {
    at: String,
    kind: String,
    cell: String,
    n: Option<i64>,
    value: Option<f64>,
    source: String,
}

pub(super) fn read(rows: &[Value]) -> Vec<Row> {
    rows.iter()
        .map(|row| Row {
            at: plain(&row["at"]),
            kind: plain(&row["kind"]),
            cell: if row["cell"].is_null() { "None".into() } else { plain(&row["cell"]) },
            n: row["n"].as_i64(),
            value: row["value"].as_f64(),
            source: plain(&row["source"]),
        })
        .collect()
}

pub fn stamp(text: &str) -> Option<f64> {
    crate::foundation::clock::Timestamp::parse_python_datetime(text).map(|at| at.unix_seconds_f64())
}

fn fallback(row: &Row, now: f64) -> Option<String> {
    let floor = match row.kind.as_str() {
        "tier" => default("min_runs"),
        "box" => default("box_calibration_n"),
        _ => default("calibration_thin_n"),
    } as i64;
    let n = row.n.unwrap_or(0);
    if n < floor {
        return Some(format!("n={n} < {floor}"));
    }
    let days = default("calibration_stale_days") as i64;
    (((now - stamp(&row.at)?) / 86_400.0).floor() as i64 >= days).then(|| format!("older than {days} days"))
}

fn latest(rows: &[Row]) -> BTreeMap<&str, Vec<&Row>> {
    let mut newest: BTreeMap<&str, &str> = BTreeMap::new();
    for row in rows.iter().filter(|row| !GOAL_KINDS.contains(&row.kind.as_str())) {
        let seen = newest.entry(&row.kind).or_insert(&row.at);
        if row.at.as_str() > *seen {
            *seen = &row.at;
        }
    }
    newest
        .into_iter()
        .map(|(kind, at)| {
            let mut batch: Vec<&Row> = rows.iter().filter(|row| row.kind == kind && row.at == at).collect();
            batch.sort_by(|a, b| a.cell.cmp(&b.cell));
            (kind, batch)
        })
        .collect()
}

fn prior_lines(batch: &[&Row], now: f64) -> Vec<String> {
    let settled = |row: &Row| row.n.is_some_and(|n| row.value.is_some_and(|value| n as f64 >= value));
    let count = batch.iter().filter(|row| settled(row)).count();
    let mut lines = vec![format!("  prior {} ago: {count}/{} settled", age(now - stamp(&batch[0].at).unwrap_or(now)), batch.len())];
    lines.extend(batch.iter().map(|row| format!("    {} {} n={}/{}", if settled(row) { "SETTLED" } else { "WAIT" }, row.cell, row.n.unwrap_or(0), crate::router::moment::python_g(row.value.unwrap_or(0.0)))));
    lines
}

fn kind_lines(kind: &str, batch: &[&Row], now: f64) -> Vec<String> {
    let thin: Vec<(&Row, String)> = batch.iter().filter_map(|row| Some((*row, fallback(row, now)?))).collect();
    let head = format!("  {kind} {} ago", age(now - stamp(&batch[0].at).unwrap_or(now)));
    if batch.len() > 4 {
        let total: i64 = batch.iter().map(|row| row.n.unwrap_or(0)).sum();
        return vec![format!("{head}: {} cells, {} calibrated, n={total}", batch.len(), batch.len() - thin.len())];
    }
    let cells: Vec<String> = batch
        .iter()
        .map(|row| {
            let n = row.n.map_or("None".into(), |n| n.to_string());
            row.value.map_or_else(|| format!("{} n={n}", row.cell), |value| format!("{} n={n} value={value:.2}", row.cell))
        })
        .collect();
    let mut lines = vec![format!("{head}: {}", cells.join(", "))];
    lines.extend(thin.iter().map(|(row, why)| format!("    {} (uncalibrated: {why})", row.cell)));
    lines
}

pub(super) fn lines(rows: &[Row], now: f64) -> Vec<String> {
    let latest = latest(rows);
    if latest.is_empty() {
        return vec!["calibration: no calibration yet".into()];
    }
    let mut lines = vec!["calibration:".to_owned()];
    for (kind, batch) in &latest {
        lines.extend(if *kind == "prior" { prior_lines(batch, now) } else { kind_lines(kind, batch, now) });
    }
    lines
}

pub(super) fn goal_eta_lines(rows: &[Row]) -> Vec<String> {
    let mut goals: Vec<&Row> = rows.iter().filter(|row| GOAL_KINDS.contains(&row.kind.as_str())).collect();
    goals.sort_by(|a, b| a.at.cmp(&b.at));
    let mut latest: BTreeMap<(&str, &str), &Row> = BTreeMap::new();
    for row in goals {
        latest.insert((&row.cell, &row.kind), row);
    }
    if latest.is_empty() {
        return vec!["goal ETA: none predicted yet".into()];
    }
    let mut lines = vec!["goal ETA:".to_owned()];
    for ((project, kind), row) in latest {
        let detail: Value = serde_json::from_str(&row.source).unwrap_or(Value::Null);
        lines.push(match (kind, row.n) {
            ("goal_eta", None | Some(0)) => format!("  {project} predicted at {}: no open goal boxes (target {})", row.at, plain(&detail["target"])),
            ("goal_eta", Some(boxes)) => format!(
                "  {project} predicted at {}: finish {} vs target {}, {boxes} boxes at width {}",
                row.at, plain(&detail["predicted_finish"]), plain(&detail["target"]), plain(&detail["width"])
            ),
            _ => format!(
                "  {project} finished {} vs predicted {} (actual/predicted {}, predicted at {})",
                plain(&detail["actual_finish"]), plain(&detail["predicted_finish"]), row.value.map_or("?".into(), |ratio| format!("{ratio:.2}")), plain(&detail["predicted_at"])
            ),
        });
    }
    lines
}
