use serde_json::Value;
use std::collections::BTreeMap;

use crate::platform::rolling_issue::Tracker;

const TITLE: &str = "main is red";
const BOX: &str = "- [ ] The newest Harness and Cargo runs on main are green";
const GATING: [&str; 2] = ["Cargo", "Harness"];

fn culprit_heads(body: &str) -> Vec<&str> {
    body.lines()
        .filter_map(|line| line.strip_prefix("Culprit range: "))
        .filter_map(|range| range.split(',').next())
        .map(str::trim)
        .filter(|head| !head.is_empty())
        .collect()
}

struct Run {
    success: bool,
    sha: String,
    url: String,
}

fn newest(runs: &str) -> Result<BTreeMap<String, Run>, String> {
    let rows: Vec<Value> = serde_json::from_str(runs).map_err(|error| format!("main-red: {error}"))?;
    let mut newest = BTreeMap::new();
    for row in rows.iter().filter(|row| row["status"] == "completed" && row["conclusion"] != "cancelled" && row["conclusion"] != "skipped") {
        let name = row["workflowName"].as_str().ok_or("main-red: workflowName missing")?;
        if !GATING.contains(&name) {
            continue;
        }
        newest.entry(name.to_owned()).or_insert_with(|| Run {
            success: row["conclusion"] == "success",
            sha: row["headSha"].as_str().unwrap_or_default().to_owned(),
            url: row["url"].as_str().unwrap_or_default().to_owned(),
        });
    }
    Ok(newest)
}

pub(super) fn settle(tracker: &mut dyn Tracker, north_runs: &str) -> Result<(), String> {
    let Some(issue) = tracker.open(TITLE)? else { return Ok(()) };
    let heads = culprit_heads(&issue.body);
    let newest = newest(north_runs)?;
    if heads.is_empty() || newest.len() < GATING.len() || newest.values().any(|run| !run.success || heads.contains(&run.sha.as_str())) {
        return Ok(());
    }
    let links: Vec<String> = newest.iter().map(|(workflow, run)| format!("[{workflow}]({}) on {}", run.url, run.sha)).collect();
    tracker.edit(issue.number, &issue.body.replacen(BOX, &BOX.replacen("[ ]", "[x]", 1), 1))?;
    tracker.close(issue.number, &format!("Main is green again: {}.", links.join(", ")))
}
