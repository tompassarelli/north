//! The lander's own queue: tickets waiting in `land-queue/<origin key>/`, the newest landing it
//! recorded and the candidate check it is running, read per repository so a stalled lander shows
//! even when GitHub has nothing pending; a queue holding lanes while origin/main stays unmoved past its limit is stalled.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use north_base::land_ticket::{self, Phase};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::DocumentMut;

static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));

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
    pub check_after_landing: bool,
    pub check_p90_s: Option<f64>,
    pub main_at: Option<f64>,
    pub last_line: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Reading {
    pub depth: usize,
    pub oldest_minutes: Option<f64>,
    pub last_landing_minutes: Option<f64>,
    pub check_minutes: Option<f64>,
    pub check_limit_minutes: Option<f64>,
    pub idle_minutes: Option<f64>,
    pub stall_limit_minutes: f64,
    pub stalled: bool,
    pub last_line: Option<String>,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
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

fn last_line(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    Some(line.chars().filter(|c| !c.is_control()).take(300).collect())
}

pub fn read(directory: &Path, now: f64) -> Facts {
    let tickets: Vec<PathBuf> = listed(directory, "json").into_iter().chain(listed(&directory.join(OUTBOX), "json")).collect();
    let oldest_queued_at = tickets.iter().filter_map(|path| started_at(path)).reduce(f64::min);
    let records = land_ticket::recorded(directory);
    let last_landed_at = listed(&directory.join("done"), "json")
        .into_iter()
        .filter(|path| {
            let ticket = fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
            land_ticket::read(records.as_ref(), directory, path, ticket.as_ref().and_then(|ticket| ticket["verdict"].as_str())) == Phase::Landed
        })
        .filter_map(|path| modified(&path))
        .reduce(f64::max);
    let mut logs: Vec<(f64, PathBuf)> = listed(&directory.join("logs"), "log").into_iter().filter_map(|path| Some((started_at(&path)?, path))).collect();
    logs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let running = logs.last().filter(|(_, path)| !finished(path));
    let check_started_at = running.map(|(started, _)| *started);
    let check_after_landing = running.is_some_and(|(_, path)| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("post-land-")));
    let mut durations: Vec<f64> = logs.iter().filter(|(started, path)| now - started <= HISTORY_S && finished(path)).filter_map(|(started, path)| Some(modified(path)? - started)).collect();
    let newest = logs.iter().filter_map(|(_, path)| Some((modified(path)?, path))).max_by(|a, b| a.0.total_cmp(&b.0));
    Facts {
        now,
        depth: tickets.len(),
        oldest_queued_at,
        last_landed_at,
        check_started_at,
        check_after_landing,
        check_p90_s: percentile_90(&mut durations),
        main_at: None,
        last_line: newest.and_then(|(_, path)| last_line(path)),
    }
}

pub fn judge(facts: &Facts) -> Reading {
    let minutes = |since: f64| (facts.now - since).max(0.0) / 60.0;
    let floor = default_number("lander_stall_floor_minutes");
    let multiple = default_number("lander_check_p90_multiple");
    let check_limit_minutes = facts.check_p90_s.map(|p90| multiple * p90 / 60.0);
    let stall_limit_minutes = check_limit_minutes.map_or(floor, |limit| limit.max(floor));
    let moved = facts.main_at.or(facts.last_landed_at);
    let idle_since = [moved, facts.oldest_queued_at].into_iter().flatten().reduce(f64::max);
    let idle_minutes = idle_since.filter(|_| facts.depth > 0).map(minutes);
    let mut reading = Reading {
        depth: facts.depth,
        oldest_minutes: facts.oldest_queued_at.map(minutes),
        last_landing_minutes: facts.last_landed_at.map(minutes),
        check_minutes: facts.check_started_at.map(minutes),
        check_limit_minutes,
        idle_minutes,
        stall_limit_minutes,
        stalled: idle_minutes.is_some_and(|idle| idle > stall_limit_minutes),
        last_line: facts.last_line.clone(),
        reasons: Vec::new(),
        notes: Vec::new(),
    };
    if let (true, Some(idle)) = (reading.stalled, reading.idle_minutes) {
        reading.reasons.push(format!("{} queued with no landing for {idle:.0}m > {stall_limit_minutes:.0}m", facts.depth));
    }
    if let (Some(age), Some(limit)) = (reading.check_minutes, reading.check_limit_minutes)
        && age > limit
    {
        if facts.check_after_landing {
            reading.notes.push(format!("after-landing run running {age:.0}m > {limit:.0}m ({multiple:.0} x its p90); it blocks no landing"));
        } else {
            reading.reasons.push(format!("lander candidate check running {age:.0}m > {limit:.0}m ({multiple:.0} x its p90)"));
        }
    }
    reading
}

#[cfg(test)]
#[path = "lander_tests.rs"]
mod tests;
