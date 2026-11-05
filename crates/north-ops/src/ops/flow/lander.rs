//! The lander's own queue: tickets waiting in `land-queue/<origin key>/`, the newest landing it
//! recorded and the candidate check it is running, read per repository so a stalled lander shows
//! even when GitHub has nothing pending.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::DocumentMut;

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| north_base::platform::harness::table(north_base::platform::harness::DEFAULTS).parse().expect("harness/defaults.toml parses"));

const OUTBOX: &str = "outbox";
const HISTORY_S: f64 = 7.0 * 86_400.0;
const MIN_CHECKS: usize = 5;
const VERDICT_LINE: &str = "safe-push: verdict ";

pub(super) fn default_number(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub now: f64,
    pub depth: usize,
    pub oldest_queued_at: Option<f64>,
    pub last_landed_at: Option<f64>,
    pub check_started_at: Option<f64>,
    pub check_p90_s: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Reading {
    pub depth: usize,
    pub oldest_minutes: Option<f64>,
    pub last_landing_minutes: Option<f64>,
    pub check_minutes: Option<f64>,
    pub check_limit_minutes: Option<f64>,
    pub reasons: Vec<String>,
}

fn modified(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn listed(directory: &Path, extension: &str) -> Vec<PathBuf> {
    fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_file() && path.extension().is_some_and(|found| found == extension)).collect()
}

fn started_at(path: &Path) -> Option<f64> {
    let micros: f64 = path.file_name()?.to_str()?.split('-').next()?.parse().ok()?;
    Some(micros / 1e6)
}

fn finished(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.lines().any(|line| line.starts_with(VERDICT_LINE)))
}

fn percentile_90(samples: &mut [f64]) -> Option<f64> {
    if samples.len() < MIN_CHECKS {
        return None;
    }
    samples.sort_by(f64::total_cmp);
    Some(samples[((samples.len() - 1) as f64 * 0.9).ceil() as usize])
}

pub fn read(directory: &Path, now: f64) -> Facts {
    let tickets: Vec<PathBuf> = listed(directory, "json").into_iter().chain(listed(&directory.join(OUTBOX), "json")).collect();
    let oldest_queued_at = tickets.iter().filter_map(|path| started_at(path)).reduce(f64::min);
    let last_landed_at = listed(&directory.join("done"), "json")
        .into_iter()
        .filter(|path| fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).is_some_and(|ticket| ticket["verdict"].as_str().is_some_and(|verdict| verdict.starts_with("landed "))))
        .filter_map(|path| modified(&path))
        .reduce(f64::max);
    let mut logs: Vec<(f64, PathBuf)> = listed(&directory.join("logs"), "log").into_iter().filter_map(|path| Some((started_at(&path)?, path))).collect();
    logs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let check_started_at = logs.last().filter(|(_, path)| !finished(path)).map(|(started, _)| *started);
    let mut durations: Vec<f64> = logs.iter().filter(|(started, path)| now - started <= HISTORY_S && finished(path)).filter_map(|(started, path)| Some(modified(path)? - started)).collect();
    Facts { now, depth: tickets.len(), oldest_queued_at, last_landed_at, check_started_at, check_p90_s: percentile_90(&mut durations) }
}

pub fn judge(facts: &Facts) -> Reading {
    let minutes = |since: f64| (facts.now - since).max(0.0) / 60.0;
    let idle_alarm = default_number("lander_idle_alarm_minutes");
    let multiple = default_number("lander_check_p90_multiple");
    let mut reading = Reading {
        depth: facts.depth,
        oldest_minutes: facts.oldest_queued_at.map(minutes),
        last_landing_minutes: facts.last_landed_at.map(minutes),
        check_minutes: facts.check_started_at.map(minutes),
        check_limit_minutes: facts.check_p90_s.map(|p90| multiple * p90 / 60.0),
        reasons: Vec::new(),
    };
    if facts.depth > 0 && let (Some(oldest), Some(idle)) = (reading.oldest_minutes, reading.last_landing_minutes.or(reading.oldest_minutes)) && idle > idle_alarm {
        reading.reasons.push(format!("lander tickets queued: {} waiting, oldest {oldest:.0}m, and main has not moved through the lander for {idle:.0}m > {idle_alarm:.0}m", facts.depth));
    }
    if let (Some(age), Some(limit)) = (reading.check_minutes, reading.check_limit_minutes)
        && age > limit
    {
        reading.reasons.push(format!("lander candidate check running {age:.0}m > {limit:.0}m ({multiple:.0} x its p90)"));
    }
    reading
}
