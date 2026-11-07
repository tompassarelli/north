use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use super::actual::actual_row;
use super::estimate::{Estimate, estimate, line};
use super::metrics::delivery_metrics;
use super::stamp;
use crate::foundation::clock::Timestamp;
use crate::platform::process::run_command_within;
use crate::platform::sqlite::{self, Access};
use crate::threads::calibration::box_durations;

const GH_LIMIT: Duration = Duration::from_secs(30);
const RECORD_LIMIT: Duration = Duration::from_secs(5);

/// The spawn gate's width for a project's boxes: the measured concurrency while the gate admits spawns, else 1.
pub fn gate_width(concurrency: usize) -> (usize, String) {
    let shown = crate::pacing::spawn_gate::summary();
    let open: Vec<&str> = shown["accounts"].as_array().into_iter().flatten().filter(|pair| pair[1] == true).filter_map(|pair| pair[0].as_str()).collect();
    let cpu = shown["cpu"] == true;
    let admitted = cpu && (!open.is_empty() || shown["accounts"].as_array().is_none_or(Vec::is_empty));
    let note = format!("gate: cpu {}, accounts open {}", if cpu { "open" } else { "full" }, if open.is_empty() { "none".to_owned() } else { open.join(",") });
    (if admitted { concurrency.max(1) } else { 1 }, note)
}

fn gh(arguments: &[&str]) -> Result<Value, String> {
    let mut command = Command::new("gh");
    command.args(arguments);
    let output = run_command_within(command, None, GH_LIMIT).ok_or("gh timed out or did not start")?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

/// The issues a project's goal names, else its open priority:now issues.
pub fn goal_issues(project: &str, config: &Value) -> Result<Vec<Value>, String> {
    let fields = "number,body,state,closedAt,labels,blockedBy";
    let repo = format!("tompassarelli/{project}");
    let numbers: Vec<String> = config["goal"].as_array().into_iter().flatten().filter_map(|goal| Some(goal.as_str()?.rsplit('#').next()?.to_owned())).collect();
    if numbers.is_empty() {
        let listed = gh(&["issue", "list", "--repo", &repo, "--state", "open", "--label", "priority:now", "--limit", "100", "--json", fields])?;
        return Ok(listed.as_array().cloned().unwrap_or_default());
    }
    numbers.iter().map(|number| gh(&["issue", "view", number, "--repo", &repo, "--json", fields])).collect()
}

/// Appends one goal row to the NorthThreads calibration table through `threads calibrate --write`; the error text, or None.
pub fn record(row: &Value, north: &Path, warnings: &mut dyn std::io::Write) -> Option<String> {
    let mut command = Command::new(north);
    command.args(["threads", "calibrate", "--write", "--goal-row", &row.to_string()]);
    let Some(output) = run_command_within(command, None, RECORD_LIMIT) else {
        let _ = writeln!(warnings, "warning: threads calibrate --write timed out after 5s; goal ETA row not recorded");
        return None;
    };
    if output.status.success() {
        return None;
    }
    let why = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Some(if why.is_empty() { format!("exit {}", output.status.code().unwrap_or(-1)) } else { why })
}

pub fn north_binary() -> std::path::PathBuf {
    std::env::var_os("NORTH_BIN").map(Into::into).or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| "north".into())
}

pub fn snapshot(database: &Path) -> Result<Vec<Value>, String> {
    sqlite::rows(database, Access::ReadOnly, Duration::from_secs(10), "SELECT * FROM runs").map_err(|error| error.to_string())
}

pub fn calibration_rows(database: &Path) -> Vec<Value> {
    sqlite::rows(database, Access::ReadOnly, Duration::from_secs(10), "SELECT at, kind, cell, n, p25, median, p75, value, source FROM calibration").unwrap_or_default()
}

fn estimate_project(project: &str, issues: &[Value], database: &Path, now: Timestamp) -> Result<(Estimate, String), String> {
    let rows = snapshot(database)?;
    let metrics = delivery_metrics(&rows, Some(project));
    let (width, note) = gate_width(delivery_metrics(&rows, None).concurrency);
    let measure = |facets: &[String]| box_durations(database, now, facets, None).map_err(|failure| match failure {
        crate::threads::verb::Failure::Usage(why) | crate::threads::verb::Failure::Refused(why) => why,
    });
    let result = estimate(project, issues, &measure, metrics.median_brief_to_landed_min, width, now)?;
    Ok((result, note))
}

/// The goal_eta row for an estimate quoted against a target still ahead of `now`; Err names a target already past.
pub fn prediction_row(project: &str, config: &Value, result: &Estimate, now: Timestamp) -> Result<Option<Value>, String> {
    let Some(target) = target_stamp(config) else { return Ok(None) };
    if target < now {
        return Err(format!("target {} has passed; set a current target", target.iso()));
    }
    let open: Vec<&str> = result.detail.iter().map(|found| found.key.as_str()).collect();
    let etas: serde_json::Map<String, Value> = result.detail.iter().map(|found| (found.key.clone(), json!(found.eta))).collect();
    let source = json!({"predicted_finish": result.finish, "boxes": result.boxes, "width": result.width, "target": target.iso(), "open": open, "etas": etas}).to_string();
    Ok(Some(json!({"kind": "goal_eta", "cell": project, "n": result.boxes, "value": result.minutes, "source": source})))
}

fn scored(project: &str, issues: &[Value], database: &Path, now: Timestamp) -> Option<String> {
    let row = actual_row(project, &calibration_rows(database), issues, now)?;
    let failed = record(&row, &north_binary(), &mut std::io::stderr());
    let source: Value = serde_json::from_str(row["source"].as_str().unwrap_or("{}")).unwrap_or_default();
    let ratio = row["value"].as_f64().map_or("n/a".to_owned(), |ratio| format!("{ratio:.2}x"));
    let kind = if source["partial"] == true { "partial" } else { "closed" };
    Some(match failed {
        None => format!("goal {project}: scored {kind}, {} boxes landed at {ratio} of predicted", row["n"]),
        Some(failed) => format!("goal {project}: actual not recorded ({failed})"),
    })
}

/// One line per project that has a target: its ETA against the target; unless `write` is off, the landed boxes scored as a
/// goal_actual row and the prediction recorded while its target is ahead.
pub fn plan_lines(cfg: &Value, database: &Path, now: Timestamp, write: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut projects: Vec<(&String, &Value)> = cfg["projects"].as_object().into_iter().flatten().filter(|(_, config)| config.get("target").is_some()).collect();
    projects.sort_by(|left, right| left.0.cmp(right.0));
    for (project, config) in projects {
        let estimated = goal_issues(project, config).and_then(|issues| Ok((estimate_project(project, &issues, database, now)?, issues)));
        match estimated {
            Err(error) => lines.push(format!("goal {project}: ETA n/a ({error})")),
            Ok(((result, note), issues)) => {
                lines.push(format!("{}; {note}", line(&result, config["target"].as_str())));
                if !write {
                    continue;
                }
                lines.extend(scored(project, &issues, database, now));
                if result.minutes.is_none() || result.boxes == 0 {
                    continue;
                }
                let failed = match prediction_row(project, config, &result, now) {
                    Err(refused) => Some(refused),
                    Ok(row) => row.and_then(|row| record(&row, &north_binary(), &mut std::io::stderr())),
                };
                lines.extend(failed.map(|failed| format!("goal {project}: prediction not recorded ({failed})")));
            }
        }
    }
    lines
}

pub fn target_stamp(config: &Value) -> Option<Timestamp> {
    stamp(config["target"].as_str()?)
}
