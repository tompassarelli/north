//! The lander's idle-slot SLO: no lane waits more than `lander_idle_slot_wait_minutes` while a
//! check slot is idle. Each queued or decided ticket waits from the time in its name until a check
//! that carries it starts; a check occupies a slot from the time in its log's name to the log's last
//! write. Breaches count per UTC day against `lander_idle_slot_breach_budget`, and `--stall-alarm`
//! tells lander-lead once on the day the budget is spent.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::alerts::{Action, Record, State, deliver};
use super::host::{Host, Places, iso};
use super::lander::default_number;

const DAY: f64 = 86_400.0;
const LEAD: &str = "lander-lead";
const KIND: &str = "slo";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Breach {
    pub lane: String,
    pub queued_at: String,
    pub breached_at: String,
    pub wait_minutes: f64,
    pub idle_minutes: f64,
    pub idle_slots: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Budget {
    pub day: String,
    pub breaches: usize,
    pub budget: usize,
    pub spent: bool,
    pub slots: usize,
    pub limit_minutes: f64,
}

#[derive(Clone, Copy, Debug)]
struct Check {
    start: f64,
    end: f64,
}

struct Lane {
    name: String,
    stem: String,
    queued: f64,
    decided: Option<f64>,
}

fn modified(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn listed(directory: &Path, extension: &str) -> Vec<PathBuf> {
    fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_file() && path.extension().is_some_and(|found| found == extension)).collect()
}

fn named_at(name: &str) -> Option<f64> {
    let micros: f64 = name.split('-').next()?.parse().ok()?;
    Some(micros / 1e6)
}

fn lane(path: &Path, decided: bool) -> Option<Lane> {
    let stem = path.file_stem()?.to_str()?.to_owned();
    let queued = named_at(&stem)?;
    let ticket: Option<Value> = fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok());
    let branch = ticket.as_ref().and_then(|ticket| ticket["branch"].as_str()).filter(|branch| !branch.is_empty());
    let name = branch.map_or_else(|| stem.split_once('-').map_or(stem.as_str(), |(_, rest)| rest).to_owned(), str::to_owned);
    Some(Lane { name, stem, queued, decided: if decided { modified(path) } else { None } })
}

fn carries_every_queued_lane(name: &str) -> bool {
    let rest = name.split_once('-').map_or("", |(_, rest)| rest);
    rest == "candidate.log" || rest.strip_prefix("speculative-").is_some_and(|level| level.trim_end_matches(".log").parse::<u32>().is_ok())
}

fn started(lane: &Lane, logs: &[(String, Check)]) -> Option<f64> {
    logs.iter().filter(|(name, check)| check.start >= lane.queued && (carries_every_queued_lane(name) || name.contains(&lane.stem))).map(|(_, check)| check.start).reduce(f64::min)
}

fn occupancy(checks: &[Check], at: f64) -> usize {
    checks.iter().filter(|check| check.start <= at && at < check.end).count()
}

fn judge(lane: &Lane, end: f64, checks: &[Check], slots: usize, limit_s: f64) -> Option<(f64, Breach)> {
    let mut edges: Vec<f64> = checks.iter().flat_map(|check| [check.start, check.end]).filter(|edge| *edge > lane.queued && *edge < end).collect();
    edges.push(lane.queued);
    edges.push(end);
    edges.sort_by(f64::total_cmp);
    edges.dedup();
    let (mut idle, mut most, mut breached) = (0.0, 0, None);
    for pair in edges.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let free = slots.saturating_sub(occupancy(checks, from));
        if free == 0 {
            continue;
        }
        most = most.max(free);
        if breached.is_none() && idle + (to - from) > limit_s {
            breached = Some(from + limit_s - idle);
        }
        idle += to - from;
    }
    let breached = breached?;
    Some((breached, Breach { lane: lane.name.clone(), queued_at: iso(lane.queued), breached_at: iso(breached), wait_minutes: (end - lane.queued) / 60.0, idle_minutes: idle / 60.0, idle_slots: most }))
}

fn day_start(now: f64) -> f64 {
    (now / DAY).floor() * DAY
}

/// Today's breaches in the queue at `directory`, judged against `slots` check slots and a wait limit in minutes.
pub fn breaches(directory: &Path, now: f64, slots: usize, limit_minutes: f64) -> Vec<Breach> {
    let today = day_start(now);
    let logs: Vec<(String, Check)> = listed(&directory.join("logs"), "log")
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            let start = named_at(&name)?;
            Some((name, Check { start, end: modified(&path).unwrap_or(start).max(start) }))
        })
        .collect();
    let checks: Vec<Check> = logs.iter().map(|(_, check)| *check).collect();
    let lanes = listed(directory, "json").into_iter().filter_map(|path| lane(&path, false)).chain(listed(&directory.join("done"), "json").into_iter().filter_map(|path| lane(&path, true)));
    let mut found: Vec<(f64, Breach)> = lanes
        .filter_map(|lane| {
            let end = started(&lane, &logs).into_iter().chain(lane.decided).chain([now]).reduce(f64::min)?.max(lane.queued);
            if end < today {
                return None;
            }
            let (at, breach) = judge(&lane, end, &checks, slots, limit_minutes * 60.0)?;
            (at >= today && at <= now).then_some((at, breach))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.lane.cmp(&b.1.lane)));
    found.into_iter().map(|(_, breach)| breach).collect()
}

/// The `idle_slot_wait_breaches` and `idle_slot_budget` fields of a flow row, with the tuned defaults.
pub fn reading(directory: &Path, now: f64) -> (Vec<Breach>, Budget) {
    let slots = default_number("lander_check_slots") as usize;
    let limit_minutes = default_number("lander_idle_slot_wait_minutes");
    let budget = default_number("lander_idle_slot_breach_budget") as usize;
    let found = breaches(directory, now, slots, limit_minutes);
    let day = iso(day_start(now)).chars().take(10).collect();
    let summary = Budget { day, breaches: found.len(), budget, spent: found.len() >= budget, slots, limit_minutes };
    (found, summary)
}

fn text(repo: &str, budget: &Budget, breaches: &[Breach]) -> String {
    let listed: Vec<String> = breaches.iter().map(|breach| format!("{} waited {:.0}m, {:.0}m of it with {} idle", breach.lane, breach.wait_minutes, breach.idle_minutes, breach.idle_slots)).collect();
    format!(
        "Lander SLO budget spent in {repo} on {}: {} lanes waited over {:.0}m while a check slot was idle (budget {} a day, {} slots): {}",
        budget.day,
        budget.breaches,
        budget.limit_minutes,
        budget.budget,
        budget.slots,
        listed.join("; ")
    )
}

/// One message to lander-lead per repository on the day its budget is spent; other days' keys are dropped.
pub fn budget_plan(rows: &[Value], previous: &State, now: f64) -> (State, Vec<Action>) {
    let mut state = previous.clone();
    let mut actions = Vec::new();
    for row in rows {
        let repo = row["repo"].as_str().unwrap_or_default();
        let (Ok(budget), Ok(breaches)) = (serde_json::from_value::<Budget>(row["idle_slot_budget"].clone()), serde_json::from_value::<Vec<Breach>>(row["idle_slot_wait_breaches"].clone())) else {
            continue;
        };
        let key = format!("{KIND}:{}:fleet:{LEAD}", budget.day);
        let record = state.entry(repo.to_owned()).or_insert_with(|| Record { since: now, sent: BTreeMap::new(), breaches: Default::default() });
        record.sent.retain(|sent, _| *sent == key);
        if budget.spent && !record.sent.contains_key(&key) {
            actions.push(Action { repo: repo.into(), key, node: LEAD.into(), channel: "fleet", text: text(repo, &budget, &breaches) });
        }
    }
    (state, actions)
}

pub fn budget_alert(places: &Places, host: &dyn Host, rows: &[Value], now: f64) -> Result<(), String> {
    deliver(places, host, "slo.json", |previous, _| budget_plan(rows, previous, now), now)
}
