//! The lander's own queue: tickets waiting in `land-queue/<origin key>/`, the newest landing it
//! recorded and the candidate check it is running, read per repository so a stalled lander shows
//! even when GitHub has nothing pending; a queue holding lanes while origin/main stays unmoved past its limit is stalled.
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use north_base::land_ticket::{self, LandTicket, Phase};
use north_base::lifecycle::Store;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::DocumentMut;

use super::write_atomic;

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

/// How far a check log was read and whether a verdict line was in it; a log that holds one always will, so only the bytes a log gained since are read.
#[derive(Default, Deserialize, Serialize)]
struct Scanned {
    offset: u64,
    found: bool,
}

type Verdicts = BTreeMap<String, Scanned>;

fn finished(path: &Path, seen: &mut Verdicts) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else { return false };
    let entry = seen.entry(name.to_owned()).or_default();
    if entry.found {
        return true;
    }
    let Ok(mut file) = fs::File::open(path) else { return false };
    if file.metadata().is_ok_and(|meta| meta.len() < entry.offset) {
        entry.offset = 0;
    }
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(entry.offset)).and_then(|_| file.read_to_end(&mut bytes)).is_err() {
        return false;
    }
    entry.found = String::from_utf8_lossy(&bytes).lines().any(|line| line.starts_with(VERDICT_LINE));
    entry.offset += bytes.iter().rposition(|byte| *byte == b'\n').map_or(0, |last| last + 1) as u64;
    entry.found
}

fn checking_started_at(store: &Store, queue: &Path) -> Option<f64> {
    let prefix = land_ticket::instance(queue, OsStr::new(""));
    store.instances_under::<LandTicket>(&prefix).ok()?.into_iter().filter(|record| record.state == Phase::Checking).map(|record| record.since.unix_seconds_f64()).reduce(f64::min)
}

fn percentile_90(samples: &mut [f64]) -> Option<f64> {
    if samples.len() < MIN_CHECKS {
        return None;
    }
    samples.sort_by(f64::total_cmp);
    Some(samples[((samples.len() - 1) as f64 * 0.9).ceil() as usize])
}

fn last_line(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut chunk = 64 * 1024;
    loop {
        let start = length.saturating_sub(chunk);
        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(start)).and_then(|_| file.read_to_end(&mut bytes)).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines = text.lines().rev().filter(|line| !line.trim().is_empty());
        let line = lines.next();
        if start > 0 && line.is_none_or(|line| text.starts_with(line)) {
            chunk *= 4;
            continue;
        }
        return Some(line?.chars().filter(|c| !c.is_control()).take(300).collect());
    }
}

pub fn read(directory: &Path, now: f64, state: &Path) -> Facts {
    let tickets: Vec<PathBuf> = listed(directory, "json").into_iter().chain(listed(&directory.join(OUTBOX), "json")).collect();
    let oldest_queued_at = tickets.iter().filter_map(|path| started_at(path)).reduce(f64::min);
    let records = land_ticket::recorded(directory);
    let mut done: Vec<(f64, PathBuf)> = listed(&directory.join("done"), "json").into_iter().filter_map(|path| Some((modified(&path)?, path))).collect();
    done.sort_by(|left, right| right.0.total_cmp(&left.0));
    let last_landed_at = done.into_iter().find_map(|(at, path)| {
        let ticket = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
        (land_ticket::read(records.as_ref(), directory, &path, ticket.as_ref().and_then(|ticket| ticket["verdict"].as_str())) == Phase::Landed).then_some(at)
    });
    let check_started_at = records.as_ref().and_then(|store| checking_started_at(store, directory));
    let logs: Vec<(f64, PathBuf)> = listed(&directory.join("logs"), "log").into_iter().filter_map(|path| Some((started_at(&path)?, path))).collect();
    let verdicts = state.join(format!("lander-verdicts-{}.json", directory.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned())));
    let mut seen: Verdicts = fs::read(&verdicts).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let before = serde_json::to_vec(&seen).unwrap_or_default();
    let mut durations: Vec<f64> = logs.iter().filter(|(started, path)| now - started <= HISTORY_S && finished(path, &mut seen)).filter_map(|(started, path)| Some(modified(path)? - started)).collect();
    let present: HashSet<&str> = logs.iter().filter_map(|(_, path)| path.file_name()?.to_str()).collect();
    seen.retain(|name, _| present.contains(name.as_str()));
    let after = serde_json::to_vec(&seen).unwrap_or_default();
    if before != after {
        let _ = fs::create_dir_all(state);
        let _ = write_atomic(&verdicts, &String::from_utf8_lossy(&after));
    }
    let newest = logs.iter().filter_map(|(_, path)| Some((modified(path)?, path))).max_by(|a, b| a.0.total_cmp(&b.0));
    Facts {
        now,
        depth: tickets.len(),
        oldest_queued_at,
        last_landed_at,
        check_started_at,
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
    };
    if let (true, Some(idle)) = (reading.stalled, reading.idle_minutes) {
        reading.reasons.push(format!("{} queued with no landing for {idle:.0}m > {stall_limit_minutes:.0}m", facts.depth));
    }
    if let (Some(age), Some(limit)) = (reading.check_minutes, reading.check_limit_minutes)
        && age > limit
    {
        reading.reasons.push(format!("lander candidate check running {age:.0}m > {limit:.0}m ({multiple:.0} x its p90)"));
    }
    reading
}

#[cfg(test)]
#[path = "lander_tests.rs"]
mod tests;
