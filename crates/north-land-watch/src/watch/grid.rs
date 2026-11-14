//! The remote-execution grid as this host's Buildbarn worker reports it: when every slot of the worker fails its
//! readiness check for a minute, the host serves no action, and the page names the worker's own cause line.
use std::collections::BTreeMap;
use std::process::Command;

use super::{Alert, Detail, Severity, WatchContext};

const WORKER_UNIT: &str = "buildbarn-worker.service";
const RUNNER_UNITS: &str = "buildbarn-runner-*";
const JOURNAL_WINDOW: &str = "--since=-15min";
const READINESS: &str = "Worker failed readiness check";
const CAUSE_CHARS: usize = 160;
const READY_FRESH: f64 = 30.0;
const ZERO_READY_SECONDS: f64 = 60.0;
pub const LOST: &str = "north grid: ";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub host: String,
    pub runners: usize,
    pub failures: Vec<Failure>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Failure {
    pub at: f64,
    pub slot: String,
    pub cause: String,
}

pub(super) fn probe(context: &WatchContext) -> Vec<Alert> {
    zero_ready(&context.grid, context.now as f64).into_iter().collect()
}

pub fn gather() -> Grid {
    let runners = listed(&["systemctl", "list-units", "--all", "--plain", "--no-legend", "--no-pager", RUNNER_UNITS]).iter().filter(|line| line.contains(".service")).count();
    let mut failures: Vec<Failure> = if runners == 0 { Vec::new() } else { listed(&["journalctl", "-u", WORKER_UNIT, JOURNAL_WINDOW, "-o", "short-unix", "--no-pager"]).iter().filter_map(|line| parse_failure(line)).collect() };
    failures.sort_by(|earlier, later| earlier.at.total_cmp(&later.at));
    Grid { host: north_base::platform::host::name(), runners, failures }
}

fn listed(argv: &[&str]) -> Vec<String> {
    Command::new("timeout").arg("30").args(argv).output().map(|output| String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()).unwrap_or_default()
}

pub fn parse_failure(line: &str) -> Option<Failure> {
    let start = line.find(READINESS)?;
    let at = line.split_whitespace().next()?.parse().ok()?;
    let slot = line.split("\"slot\":\"").nth(1)?.split('"').next()?.to_owned();
    let cause = line[start + READINESS.len()..].trim_start_matches(':').trim().chars().take(CAUSE_CHARS).collect();
    Some(Failure { at, slot, cause })
}

/// When every slot last failed its readiness check within `READY_FRESH` of each other and of `now`, the moment that
/// streak began.
pub fn zero_ready_since(grid: &Grid, now: f64) -> Option<f64> {
    if grid.runners == 0 {
        return None;
    }
    let mut last: BTreeMap<&str, f64> = BTreeMap::new();
    let mut streak: Option<f64> = None;
    for failure in &grid.failures {
        last.insert(failure.slot.as_str(), failure.at);
        let together = last.len() == grid.runners && last.values().all(|seen| failure.at - seen <= READY_FRESH);
        streak = if together { streak.or(Some(failure.at)) } else { None };
    }
    let current = last.len() == grid.runners && last.values().all(|seen| now - seen <= READY_FRESH);
    streak.filter(|_| current)
}

fn zero_ready(grid: &Grid, now: f64) -> Option<Alert> {
    let since = zero_ready_since(grid, now)?;
    if now - since < ZERO_READY_SECONDS {
        return None;
    }
    let cause = grid.failures.last().map_or("", |failure| failure.cause.as_str());
    let summary = format!("{LOST}no ready remote-execution worker on {} for {:.0} s, all {} slots failing their readiness check: {cause}", grid.host, now - since, grid.runners);
    Some(Alert { key: format!("grid:zero-ready:{}", grid.host), severity: Severity::Page, repo: None, owner: None, summary, since: since as u64, detail: Detail::default() })
}

#[cfg(test)]
#[path = "grid_tests.rs"]
mod tests;
