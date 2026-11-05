use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::boxes::parse_boxes;
use super::stamp;
use crate::foundation::clock::Timestamp;

fn closed_at(issue: &Value) -> Option<Timestamp> {
    issue["state"].as_str().unwrap_or("").eq_ignore_ascii_case("CLOSED").then(|| issue["closedAt"].as_str().and_then(stamp)).flatten()
}

fn rows_of<'a>(calibration: &'a [Value], kind: &'a str, project: &'a str) -> impl Iterator<Item = (&'a Value, Timestamp)> + 'a {
    calibration.iter().filter(move |row| row["kind"] == kind && row["cell"].as_str() == Some(project)).filter_map(|row| Some((row, stamp(row["at"].as_str()?)?)))
}

/// The goal_actual row for the boxes landed since the first goal_eta prediction after the project's previous goal_actual, or None
/// when nothing was predicted or no predicted box has landed. A closed goal scores every predicted box against its last issue close;
/// an open goal scores the landed boxes alone against `now`, weighing the predicted minutes by the landed share of predicted box minutes.
pub fn actual_row(project: &str, calibration: &[Value], issues: &[Value], now: Timestamp) -> Option<Value> {
    let since = rows_of(calibration, "goal_actual", project).map(|(_, at)| at).max();
    let (first, first_at) = rows_of(calibration, "goal_eta", project).filter(|(_, at)| since.is_none_or(|since| *at >= since)).min_by_key(|(_, at)| *at)?;
    let detail: Value = serde_json::from_str(first["source"].as_str()?).ok()?;
    let complete = !issues.is_empty() && issues.iter().all(|issue| closed_at(issue).is_some());
    let open: Vec<&str> = detail["open"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let unlanded: Vec<Value> = issues.iter().filter(|issue| closed_at(issue).is_none()).cloned().collect();
    let still: BTreeSet<String> = parse_boxes(project, &unlanded).into_iter().map(|found| found.key).collect();
    let landed: Vec<&str> = open.iter().copied().filter(|key| !still.contains(*key)).collect();
    if !complete && landed.is_empty() {
        return None;
    }
    let eta = |key: &str| detail["etas"][key].as_f64().unwrap_or(0.0);
    let planned: f64 = open.iter().map(|key| eta(key)).sum();
    let share = if complete || planned == 0.0 { 1.0 } else { landed.iter().map(|key| eta(key)).sum::<f64>() / planned };
    let actual = if complete { issues.iter().filter_map(closed_at).max()? } else { now };
    let minutes = (actual.unix_nanos() - first_at.unix_nanos()) as f64 / 60e9;
    let predicted = first["value"].as_f64().map(|value| value * share).filter(|value| *value != 0.0);
    let scored = if complete && open.is_empty() { first["n"].as_i64().unwrap_or_default() } else { landed.len() as i64 };
    Some(json!({
        "kind": "goal_actual", "cell": project, "n": scored,
        "value": predicted.map(|predicted| minutes / predicted),
        "source": json!({
            "predicted_at": first["at"], "predicted_finish": detail["predicted_finish"], "target": detail["target"], "partial": !complete,
            "landed": landed, "boxes": first["n"], "predicted_minutes": predicted.map(|predicted| (predicted * 10.0).round() / 10.0),
            "actual_finish": actual.iso(), "actual_minutes": (minutes * 10.0).round() / 10.0,
        }).to_string(),
    }))
}
