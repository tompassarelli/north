use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use super::actual::actual_row;
use super::plan::{calibration_rows, goal_issues, north_binary, record, snapshot};
use super::{plus_minutes, read_config, snapshot_path, state_dir, stamp};
use crate::foundation::clock::Timestamp;

static CHECKBOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[([ xX])\]").expect("checkbox pattern"));

fn verdict(pass: bool) -> &'static str {
    if pass { "pass" } else { "fail" }
}

pub fn score(project: &str, target: Timestamp, now: Timestamp, issues: Option<&[Value]>, runs: &[Value], budget: Option<f64>) -> Result<Value, String> {
    if now < target {
        return Err("project target has not passed".into());
    }
    let closed = |issue: &Value| issue["state"].as_str().unwrap_or("").eq_ignore_ascii_case("CLOSED").then(|| issue["closedAt"].as_str().and_then(stamp)).flatten();
    let on_date = issues.map_or("unmeasured", |issues| {
        let complete = !issues.is_empty() && issues.iter().all(|issue| closed(issue).is_some());
        verdict(complete && issues.iter().all(|issue| closed(issue).is_some_and(|at| at <= target)))
    });
    let spec = issues.map_or("unmeasured", |issues| {
        verdict(issues.iter().all(|issue| CHECKBOX.captures_iter(issue["body"].as_str().unwrap_or("")).all(|cap| cap[1].eq_ignore_ascii_case("x"))))
    });
    let costs: Option<Vec<f64>> = runs.iter().map(|run| run["cost_usd"].as_f64()).collect();
    let spent = costs.filter(|costs| !costs.is_empty()).map(|costs| costs.iter().sum::<f64>());
    let in_budget = match (spent, budget) {
        (Some(spent), Some(budget)) => verdict(spent <= budget),
        _ => "unmeasured",
    };
    Ok(json!({
        "project": project, "target": target.iso().replace('Z', "+00:00"), "scored_at": now.iso_micros_utc_offset(),
        "on-date": on_date, "in-budget": in_budget, "spec-pass": spec, "spent_usd": spent, "budget_usd": budget,
    }))
}

fn budget(harness: &Path, config_path: &Path, database: &Path, share: Option<f64>, now: Timestamp) -> Option<f64> {
    Some(super::super::status::available_usd(harness, config_path, database, now).ok()?? * share? / 100.0)
}

fn week_runs(runs: Vec<Value>, project: &str, target: Timestamp) -> Vec<Value> {
    let since = plus_minutes(target, -7.0 * 1440.0);
    let item = Regex::new(&format!(r"(?:^|\s){}#\d+\b", regex::escape(project))).expect("item pattern");
    runs.into_iter().filter(|run| run["ended"].as_str().and_then(stamp).is_some_and(|ended| since <= ended && ended <= target) && item.is_match(run["item"].as_str().unwrap_or(""))).collect()
}

fn run_score(harness: &Path, project: &str, config_path: &Path) -> Result<(), String> {
    let cfg = read_config(config_path)?;
    let config = cfg["projects"].get(project).ok_or_else(|| project.to_owned())?;
    let target = config["target"].as_str().and_then(stamp).ok_or("project has no target")?;
    let now = Timestamp::now();
    if now < target {
        return Err("project target has not passed".into());
    }
    let database = snapshot_path();
    if database.file_name().is_some_and(|name| name == "threads.db") {
        return Err("quotes require the mirrored snapshot.db, not threads.db".into());
    }
    let runs = week_runs(snapshot(&database).unwrap_or_default(), project, target);
    let issues = goal_issues(project, config).ok().filter(|issues| !issues.is_empty());
    let row = score(project, target, now, issues.as_deref(), &runs, budget(harness, config_path, &database, config["share"].as_f64(), now))?;
    let state = state_dir().join("agents/goal-scores.jsonl");
    fs::create_dir_all(state.parent().ok_or("no state directory")?).map_err(|error| error.to_string())?;
    if let Some(actual) = issues.as_deref().and_then(|issues| actual_row(project, &calibration_rows(&database), issues, now))
        && let Some(failed) = record(&actual, &north_binary(), &mut std::io::stderr())
    {
        eprintln!("{project} goal ETA actual not recorded: {failed}");
    }
    let mut stream = fs::OpenOptions::new().create(true).append(true).open(&state).map_err(|error| error.to_string())?;
    writeln!(stream, "{row}").map_err(|error| error.to_string())?;
    for name in ["on-date", "in-budget", "spec-pass"] {
        println!("{project} {name}: {}", row[name].as_str().unwrap_or("unmeasured"));
    }
    Ok(())
}

pub fn run(harness: &Path, project: &str, config_path: &Path) -> u8 {
    match run_score(harness, project, config_path) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("agents plan --score: {error}");
            1
        }
    }
}
