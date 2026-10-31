//! A flow row as the one line `north flow` prints per repository.
use serde_json::Value;

use super::host::iso;
use crate::router::choose::missed_target;
use crate::router::moment::python_g;

fn plain(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_owned)
}

fn shown(value: &Value, digits: usize, unit: &str) -> String {
    match value.as_f64() {
        Some(number) => format!("{number:.digits$}{unit}"),
        None if value.is_null() => "n/a".into(),
        None => plain(value),
    }
}

fn general(value: &Value, unit: &str) -> String {
    match value.as_f64() {
        Some(number) => format!("{}{unit}", python_g(number)),
        None if value.is_null() => "n/a".into(),
        None => plain(value),
    }
}

fn cleanup(row: &Value) -> String {
    match row["cleanup"].as_array().map(Vec::len).filter(|count| *count != 0) {
        Some(count) => format!(", cleanup {count} lanes (not a stall)"),
        None => String::new(),
    }
}

fn stranded(row: &Value) -> String {
    match row["stranded"].as_array().map(Vec::len).filter(|count| *count != 0) {
        Some(count) => format!(", stranded {count} lanes: {}", super::stranded::lines(row).join("; ")),
        None => String::new(),
    }
}

fn dead(row: &Value) -> String {
    match (row["dead"].as_f64().filter(|count| *count != 0.0), row["dead_oldest_minutes"].as_f64()) {
        (Some(count), Some(oldest)) => format!(", dead {count} (oldest {oldest:.0}m)"),
        _ => String::new(),
    }
}

fn stall(row: &Value) -> String {
    let age = row["main_age_minutes"].as_f64().map_or_else(|| plain(&row["main_age_minutes"]), |age| format!("{age:.1}m"));
    let job = &row["oldest_job"];
    let oldest = if job.is_object() {
        format!("{:.1}m {} run {} {} on {}", job["age_minutes"].as_f64().unwrap_or(0.0), plain(&job["workflow"]), plain(&job["run_id"]), plain(&job["name"]), plain(&job["runner"]))
    } else if job.is_null() {
        "none".into()
    } else {
        plain(job)
    };
    let verdict = &row["stall"];
    let verdict = if verdict.is_object() {
        format!("{}{}", if verdict["stalled"] == true { "STALLED: " } else { "" }, plain(&verdict["reason"]))
    } else {
        plain(verdict)
    };
    format!(", main idle {age}, oldest landing job {oldest}{}, stall {verdict}", job_limits(row))
}

fn job_limits(row: &Value) -> String {
    let listed: Vec<String> = row["job_limits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|job| format!("{} run {} {} {} of {}", plain(&job["workflow"]), plain(&job["run_id"]), plain(&job["name"]), shown(&job["age_minutes"], 1, "m"), shown(&job["limit_minutes"], 1, "m")))
        .collect();
    if listed.is_empty() { String::new() } else { format!(", job limits [{}]", listed.join("; ")) }
}

fn throughput(row: &Value) -> String {
    format!(
        ", closed {} needed {}, land-to-close {}, land ratio {} over 3h, failed unowned {}, oldest lane wait {}",
        shown(&row["closed_per_hour"], 1, "/h"),
        shown(&row["needed_per_hour"], 1, "/h"),
        shown(&row["land_to_close_minutes"], 0, "m"),
        shown(&row["land_ratio_3h"], 2, ""),
        plain(&row["failed_unowned"]),
        shown(&row["oldest_lane_wait_minutes"], 0, "m"),
    )
}

/// `services` overrides the costs file the missed-target decision prices levers from.
pub fn format_row(row: &Value, services: Option<&[Value]>) -> String {
    let mut text = format!(
        "{}: pending {}{}, landed {}/h over 3h, push-to-land median {} p90 {} over 24h",
        plain(&row["repo"]),
        plain(&row["pending"]),
        dead(row) + &cleanup(row) + &stranded(row),
        general(&row["landed_per_hour"], ""),
        general(&row["push_to_land_minutes"], "m"),
        general(&row["push_to_land_p90_minutes"], "m"),
    );
    if row.get("stall").is_some() {
        text += &stall(row);
    }
    if row.get("closed_per_hour").is_some() {
        text += &throughput(row);
    }
    for guard in ["actions", "wisp", "reuse", "pins"].iter().filter_map(|kind| row.get(*kind).filter(|guard| guard.is_object())) {
        text += &format!(", {}{}", plain(&guard["text"]), if guard["reason"].as_str().is_some_and(|reason| !reason.is_empty()) { " FLAGGED" } else { "" });
    }
    if let (Some(target), Some(finish)) = (row["target"].as_f64(), row["goal_finish"].as_f64())
        && finish > target
    {
        text += &format!(", goal ETA finish {}\n{}", iso(finish), missed_target(&plain(&row["repo"]), &iso(target), services));
    }
    text
}
