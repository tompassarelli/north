use super::Finding;
use crate::platform::watch_named;
use serde_json::Value;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const RED_SECONDS: u64 = 600;
const SHORT: usize = 12;
const UNJUDGED: [&str; 3] = ["cancelled", "skipped", "neutral"];
const MAIN_RED: &str = "north main is red";
const RUNS: &str = "north/watch/runs-north.json";

#[derive(Debug, PartialEq)]
pub(super) struct Red {
    pub shas: Vec<String>,
    pub since: u64,
    pub seconds: u64,
    pub from: u64,
}

pub(super) fn unwatched(runs: &str, began: u64, now: u64, named: &dyn Fn(&str) -> bool, alerted: &dyn Fn(u64, u64) -> bool, reverted: &dyn Fn(&str) -> bool) -> Result<Vec<Red>, String> {
    let body: Value = serde_json::from_str(runs).map_err(|error| format!("{RUNS}: {error}"))?;
    let mut judged: Vec<(String, u64, bool, String)> = body["workflow_runs"]
        .as_array()
        .ok_or_else(|| format!("{RUNS} holds no workflow_runs"))?
        .iter()
        .filter(|run| run["path"].as_str().is_some_and(|path| path.ends_with("cargo.yml")) && run["head_branch"] == "main" && run["event"] == "push" && run["status"] == "completed")
        .filter_map(|run| {
            let conclusion = run["conclusion"].as_str().filter(|conclusion| !UNJUDGED.contains(conclusion))?;
            Some((run["created_at"].as_str()?.to_owned(), watch_named::run_time(run["updated_at"].as_str()?)?, conclusion == "success", run["head_sha"].as_str()?.to_owned()))
        })
        .collect();
    judged.sort();
    let oldest = body["workflow_runs"].as_array().into_iter().flatten().filter_map(|run| watch_named::run_time(run["created_at"].as_str()?)).min().unwrap_or(0);
    let mut reds: Vec<Red> = Vec::new();
    let mut red_run = false;
    for (index, (created, finished, success, sha)) in judged.iter().enumerate() {
        if *success {
            if let Some(open) = reds.last_mut().filter(|_| red_run) {
                open.seconds = finished.saturating_sub(open.since);
            }
            red_run = false;
        } else if red_run {
            if let Some(open) = reds.last_mut() {
                open.shas.push(sha.clone());
            }
        } else {
            let from = if index == 0 { oldest } else { watch_named::run_time(created).unwrap_or(*finished) };
            reds.push(Red { shas: vec![sha.clone()], since: *finished, seconds: now.saturating_sub(*finished), from });
            red_run = true;
        }
    }
    if let Some(open) = reds.last_mut().filter(|_| red_run) {
        open.seconds = now.saturating_sub(open.since);
    }
    Ok(reds.into_iter().filter(|red| red.since >= began && red.seconds > RED_SECONDS && !red.shas.iter().any(|sha| named(sha) || reverted(sha)) && !alerted(red.from, red.since.saturating_add(red.seconds))).collect())
}

fn reverted_on_origin(sha: &str) -> bool {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let args = ["log", "--format=%B", &format!("{sha}..origin/main")];
    crate::platform::git::run(&home.join("code/north/main"), &args).is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).lines().any(|line| line.starts_with("Sheriff-Reverts:")))
}

pub(super) fn finding(state: &Path, at: SystemTime) -> Finding {
    let name = "main-red";
    let failed = |verdict: String| Finding { name, verdict, failed: true };
    let now = at.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let Some(named) = watch_named::read(state) else { return failed(format!("FAIL never recorded: no {}; north ops watch has not run here", watch_named::NAMED)) };
    let runs = match std::fs::read_to_string(state.join(RUNS)) {
        Ok(runs) => runs,
        Err(error) => return failed(format!("FAIL {RUNS} unread: {error}")),
    };
    let was_named = |sha: &str| named.alerts.iter().any(|alert| alert.summary.contains(&sha[..sha.len().min(SHORT)]));
    let was_alerted = |from: u64, until: u64| named.alerts.iter().any(|alert| alert.summary.starts_with(MAIN_RED) && (from..=until).contains(&alert.at));
    match unwatched(&runs, named.began, now, &was_named, &was_alerted, &reverted_on_origin) {
        Err(error) => failed(format!("FAIL {error}")),
        Ok(reds) if reds.is_empty() => Finding { name, verdict: "ok: no main red over 10 min went unnamed and unreverted".to_owned(), failed: false },
        Ok(reds) => {
            let told: Vec<String> = reds.iter().map(|red| format!("{} red for {} min with no sheriff revert and no fleet message naming it", &red.shas[0][..red.shas[0].len().min(SHORT)], red.seconds / 60)).collect();
            failed(format!("FAIL {}", told.join("; ")))
        }
    }
}

#[cfg(test)]
#[path = "red_main_tests.rs"]
mod tests;
