use serde_json::{Value, json};

use super::stamp;

/// The goal_actual row for a finished goal: its first goal_eta prediction since the previous score against the last issue close,
/// or None while any issue is open or nothing was predicted.
pub fn actual_row(project: &str, predictions: &[Value], previous_score: Option<&str>, issues: &[Value]) -> Option<Value> {
    let closed = |issue: &Value| issue["state"].as_str().unwrap_or("").eq_ignore_ascii_case("CLOSED") && issue["closedAt"].as_str().is_some_and(|at| !at.is_empty());
    if issues.is_empty() || !issues.iter().all(closed) {
        return None;
    }
    let since = previous_score.and_then(stamp);
    let mut ours: Vec<(&Value, i128)> = predictions
        .iter()
        .filter(|row| row["kind"] == "goal_eta" && row["cell"].as_str() == Some(project))
        .filter_map(|row| Some((row, stamp(row["at"].as_str()?)?.unix_nanos())))
        .filter(|(_, at)| since.is_none_or(|since| *at > since.unix_nanos()))
        .collect();
    ours.sort_by(|left, right| left.0["at"].as_str().cmp(&right.0["at"].as_str()));
    let (first, first_at) = *ours.first()?;
    let detail: Value = serde_json::from_str(first["source"].as_str()?).ok()?;
    let actual = issues.iter().filter_map(|issue| stamp(issue["closedAt"].as_str()?)).max()?;
    let minutes = (actual.unix_nanos() - first_at) as f64 / 60e9;
    let predicted = first["value"].as_f64().filter(|value| *value != 0.0);
    Some(json!({
        "kind": "goal_actual", "cell": project, "n": first["n"],
        "value": predicted.map(|predicted| minutes / predicted),
        "source": json!({"predicted_at": first["at"], "predicted_finish": detail["predicted_finish"], "actual_finish": actual.iso(), "actual_minutes": (minutes * 10.0).round() / 10.0}).to_string(),
    }))
}
