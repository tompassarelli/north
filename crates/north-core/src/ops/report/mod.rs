//! `north report`: what the feedback loop has learned, read from the worker ledger and the
//! operator's state: calibration, escalation arms, spawn-gate denials and goal scores.
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};

mod arms;
pub use arms::wilson;
mod calibration;
#[cfg(test)]
mod tests;

const WINDOW_SECONDS: f64 = 7.0 * 86_400.0;
const GATES: [&str; 3] = ["capacity", "flow", "pace"];

fn age(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor() as i64;
    if minutes < 120 {
        format!("{minutes}m")
    } else if minutes < 2880 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", minutes / 1440)
    }
}

fn plain(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_owned)
}

fn money(value: &Value) -> String {
    value.as_f64().map_or("?".into(), |amount| format!("${amount:.2}"))
}

fn denial_lines(denials: &[Value], now: f64) -> Vec<String> {
    let since = Timestamp::from_unix_seconds_f64(now - WINDOW_SECONDS).iso();
    let dated: Vec<&Value> = denials.iter().filter(|denial| denial["at"].as_str().is_some_and(|at| !at.is_empty())).collect();
    let recent: Vec<&&Value> = dated.iter().filter(|denial| denial["at"].as_str().is_some_and(|at| at >= since.as_str())).collect();
    let counts: Vec<String> = GATES.iter().map(|gate| format!("{gate} {}", recent.iter().filter(|denial| denial["gate"] == *gate).count())).collect();
    let undated = denials.len() - dated.len();
    let tail = if undated > 0 { format!(", undated {undated} (logged before timestamps)") } else { String::new() };
    vec![format!("spawn-gate denials 7d: {} ({}){tail}", recent.len(), counts.join(", "))]
}

fn goal_lines(scores: &[Value]) -> Vec<String> {
    let mut latest: Vec<&Value> = Vec::new();
    for score in scores {
        match latest.iter().position(|seen| seen["project"] == score["project"]) {
            Some(index) => latest[index] = score,
            None => latest.push(score),
        }
    }
    if latest.is_empty() {
        return vec!["goal scores: none scored yet".into()];
    }
    latest.sort_by_key(|score| plain(&score["project"]));
    let mut lines = vec!["goal scores:".to_owned()];
    lines.extend(latest.iter().map(|score| {
        format!(
            "  {} target {}: on-date {}, in-budget {} (spent {} vs budget {}), spec {}",
            plain(&score["project"]),
            plain(&score["target"]).chars().take(10).collect::<String>(),
            plain(&score["on-date"]),
            plain(&score["in-budget"]),
            money(&score["spent_usd"]),
            money(&score["budget_usd"]),
            plain(&score["spec-pass"]),
        )
    }));
    lines
}

pub struct State<'a> {
    pub calibration: &'a [calibration::Row],
    pub denials: &'a [Value],
    pub scores: &'a [Value],
    pub runs: &'a [arms::Run],
}

pub fn render(state: &State, now: f64) -> Vec<String> {
    let mut lines = calibration::lines(state.calibration, now);
    lines.extend(arms::lines(state.runs));
    lines.extend(denial_lines(state.denials, now));
    lines.extend(goal_lines(state.scores));
    lines.extend(calibration::goal_eta_lines(state.calibration));
    lines
}

fn jsonl(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).map(|text| text.lines().filter(|line| !line.trim().is_empty()).filter_map(|line| serde_json::from_str(line).ok()).collect()).unwrap_or_default()
}

fn ledger(home: &Path, state: &Path) -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("THREADS_DB") {
        return path.into();
    }
    state.join("threads").join(if home.join(".config/threads/remote").exists() { "snapshot.db" } else { "threads.db" })
}

fn select(database: &Path, query: &str) -> Vec<Value> {
    sqlite::rows(database, Access::ReadOnly, Duration::from_secs(10), query).unwrap_or_default()
}

pub fn run(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north report");
        return 2;
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let root = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state"), std::path::PathBuf::from);
    let state = root.join("agents");
    let database = ledger(&home, &root);
    let rows = calibration::read(&select(&database, calibration::QUERY));
    let runs = arms::read(&select(&database, arms::QUERY));
    let (denials, scores) = (jsonl(&state.join("spawn-gate-denials.jsonl")), jsonl(&state.join("goal-scores.jsonl")));
    let now = Timestamp::now().unix_seconds_f64();
    println!("{}", render(&State { calibration: &rows, denials: &denials, scores: &scores, runs: &runs }, now).join("\n"));
    0
}
