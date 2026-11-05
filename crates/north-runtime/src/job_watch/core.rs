use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::foundation::clock::Timestamp;

pub(super) const GRACE: i64 = 30 * 60;
pub(super) const REPEAT: f64 = 24.0 * 3600.0;
const OPEN_LIMIT: f64 = 6.0 * 3600.0;
const ONLINE_WITHIN: f64 = 5.0 * 60.0;

pub(super) struct Entry {
    pub name: String,
    pub job: String,
    pub placement: String,
    pub deadline: i64,
    pub period: Option<i64>,
}

pub(super) struct Laptop {
    pub handshake: Option<f64>,
    pub online_since: Option<f64>,
}

pub(super) struct Alert {
    pub name: String,
    pub topic: &'static str,
    pub text: String,
    pub title: String,
}

pub(super) type State = BTreeMap<String, Value>;
pub(super) type Rows = BTreeMap<(String, String), Value>;

fn unit_seconds(unit: &str) -> i64 {
    match unit {
        "s" => 1,
        "min" | "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => 7 * 86400,
    }
}

pub(super) fn duration(text: &str) -> Option<i64> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"^\s*(\d+)\s*(s|min|m|h|d|w)\s*$").unwrap());
    let found = pattern.captures(text)?;
    found[1].parse::<i64>().ok()?.checked_mul(unit_seconds(&found[2]))
}

fn period_of(schedule: &str) -> Option<i64> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"every (\d+) ?(min|h)").unwrap());
    if let Some(found) = pattern.captures(schedule) {
        return found[1].parse::<i64>().ok()?.checked_mul(unit_seconds(&found[2]));
    }
    [("hourly", 3600), ("weekly", 7 * 86400), ("daily", 86400)]
        .into_iter()
        .find(|(word, _)| schedule.contains(word))
        .map(|(_, seconds)| seconds)
}

pub(super) fn epoch(stamp: Option<&Value>) -> Option<f64> {
    let text = stamp?.as_str().filter(|text| !text.is_empty())?;
    Timestamp::parse(text).ok().map(Timestamp::unix_seconds_f64)
}

fn fmt(time: Option<f64>) -> String {
    match time {
        None => "never".to_owned(),
        Some(seconds) => {
            let minute = Timestamp::from_unix_seconds_f64(seconds).iso_minute();
            format!("{} UTC", minute.trim_end_matches('Z').replace('T', " "))
        }
    }
}

fn span(seconds: i64) -> String {
    if seconds % 86400 == 0 {
        format!("{}d", seconds / 86400)
    } else if seconds % 3600 == 0 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}min", seconds / 60)
    }
}

fn overdue(last_ok: f64, period: Option<i64>, deadline: i64, online_since: Option<f64>, now: f64) -> Option<&'static str> {
    if now - last_ok > deadline as f64 {
        return Some("deadline");
    }
    if let (Some(period), Some(online_since)) = (period, online_since) {
        let grace = (period + GRACE) as f64;
        if now - online_since > grace && now - last_ok > grace {
            return Some("online");
        }
    }
    None
}

fn message(entry: &Entry, reason: &str, row: &Value, last_ok: Option<f64>, laptop: &Laptop, now: f64) -> String {
    let job = &entry.job;
    let mut text = if entry.placement == "nexus" {
        format!("{job} on nexus missed its deadline ({}); last success {}", span(entry.deadline), fmt(last_ok))
    } else if laptop.online_since.is_none() {
        format!("{job} missed its deadline; laptop offline since {}; it runs on wake", fmt(laptop.handshake))
    } else if reason == "online" {
        format!(
            "{job} has had no successful run for {} while the laptop has been online since {}; last success {}",
            span(entry.period.unwrap_or(0) + GRACE),
            fmt(laptop.online_since),
            fmt(last_ok)
        )
    } else {
        format!(
            "{job} missed its deadline ({}); laptop online since {}; last success {}",
            span(entry.deadline),
            fmt(laptop.online_since),
            fmt(last_ok)
        )
    };
    if let Some(open_since) = epoch(row.get("open_since")) {
        let later_than_success = epoch(row.get("last_ok")).is_none_or(|ok| open_since > ok);
        if now - open_since > OPEN_LIMIT && later_than_success {
            text.push_str(&format!("; its run started {} never finished", fmt(Some(open_since))));
        }
    }
    text
}

pub(super) fn step(entries: &[Entry], rows: &Rows, hosts: &BTreeMap<String, String>, laptop: &Laptop, state: &State, now: f64) -> (Vec<Alert>, State) {
    let mut alerts = Vec::new();
    let mut next = State::new();
    for entry in entries {
        let host = hosts.get(&entry.placement).cloned().unwrap_or_default();
        let previous = state.get(&entry.name).filter(|value| value.as_object().is_some_and(|map| !map.is_empty()));
        let Some(row) = hosts.contains_key(&entry.placement).then(|| rows.get(&(entry.job.clone(), host))).flatten() else {
            if let Some(previous) = previous {
                next.insert(entry.name.clone(), previous.clone());
            }
            continue;
        };
        let last_ok = epoch(row.get("last_ok"));
        let Some(reference) = last_ok.or_else(|| epoch(row.get("first_started"))) else { continue };
        let reason = if entry.placement == "laptop" {
            overdue(reference, entry.period, entry.deadline, laptop.online_since, now)
        } else {
            overdue(reference, None, entry.deadline, None, now)
        };
        if let Some(reason) = reason {
            let alerted = previous.and_then(|value| value.get("alerted")).and_then(Value::as_f64);
            if previous.is_none() || now - alerted.unwrap_or(0.0) >= REPEAT {
                alerts.push(Alert {
                    name: entry.name.clone(),
                    topic: "normal",
                    text: message(entry, reason, row, last_ok, laptop, now),
                    title: format!("Nexus: {} overdue", entry.job),
                });
                next.insert(entry.name.clone(), json!({"overdue": true, "alerted": now}));
            } else if let Some(previous) = previous {
                next.insert(entry.name.clone(), previous.clone());
            }
        } else if previous.is_some() {
            alerts.push(Alert {
                name: entry.name.clone(),
                topic: "normal",
                text: format!("{} recovered; last success {}", entry.job, fmt(last_ok)),
                title: format!("Nexus: {} recovered", entry.job),
            });
        }
    }
    (alerts, next)
}

pub(super) fn presence(peers: &Value, name: &str, now: f64) -> Laptop {
    let peer = peers.get("peers").and_then(|all| all.get(name));
    let number = |key: &str| peer.and_then(|peer| peer.get(key)).and_then(Value::as_f64).filter(|value| *value != 0.0);
    let handshake = number("handshake");
    let online = handshake.is_some_and(|seen| now - seen < ONLINE_WITHIN);
    Laptop { handshake, online_since: if online { number("online_since").or(handshake) } else { None } }
}

pub(super) fn read_registry(directory: &Path) -> Vec<Entry> {
    let Ok(listing) = std::fs::read_dir(directory) else { return Vec::new() };
    let mut files: Vec<_> = listing.flatten().map(|file| file.path()).filter(|path| path.extension().is_some_and(|ext| ext == "md") && path.is_file()).collect();
    files.sort();
    let mut entries = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        if !text.starts_with("---\n") {
            continue;
        }
        let head = text.split("\n---").next().unwrap_or("");
        let mut meta = Map::new();
        for line in head.lines().skip(1) {
            if let Some((key, value)) = line.split_once(':') {
                meta.insert(key.trim().to_owned(), Value::String(value.trim().to_owned()));
            }
        }
        let field = |key: &str| meta.get(key).and_then(Value::as_str);
        let placement = field("placement").unwrap_or("");
        let deadline = field("deadline").and_then(duration).filter(|seconds| *seconds != 0);
        if let (true, Some(deadline)) = (placement == "laptop" || placement == "nexus", deadline) {
            let stem = file.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
            let name = field("name").unwrap_or(stem).to_owned();
            entries.push(Entry {
                job: name.split('@').next().unwrap_or("").to_owned(),
                name,
                placement: placement.to_owned(),
                deadline,
                period: period_of(field("schedule").unwrap_or("")),
            });
        }
    }
    entries
}
