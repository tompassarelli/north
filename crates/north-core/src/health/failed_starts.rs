use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const LIMIT: usize = 3;
const DAY: u64 = 86_400;
const ERROR_WORDS: [&str; 8] = ["error", "fail", "denied", "cannot", "unable", "broken", "panic", "fatal"];

#[derive(Debug, PartialEq)]
pub(super) struct Failing {
    pub(super) unit: String,
    pub(super) count: usize,
    pub(super) error: String,
}

#[derive(Default, Deserialize, Serialize)]
pub(super) struct Told {
    #[serde(default)]
    day: u64,
    #[serde(default)]
    sent: Vec<String>,
}

#[derive(Default)]
struct Seen {
    count: usize,
    start_message: String,
    lines: Vec<String>,
}

pub(super) fn scan(journal: &str) -> Vec<Failing> {
    let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
    for entry in journal.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        let message = text(&entry["MESSAGE"]);
        if message.starts_with("Failed to start") {
            if let Some(unit) = entry["USER_UNIT"].as_str() {
                let seen = seen.entry(unit.to_owned()).or_default();
                seen.count += 1;
                seen.start_message = message;
            }
        } else if let Some(unit) = entry["_SYSTEMD_USER_UNIT"].as_str().filter(|_| entry["SYSLOG_IDENTIFIER"] != "systemd") {
            let line = message.trim();
            if !line.is_empty() {
                seen.entry(unit.to_owned()).or_default().lines.push(line.to_owned());
            }
        }
    }
    seen.into_iter()
        .filter(|(_, seen)| seen.count >= LIMIT)
        .map(|(unit, seen)| Failing { unit, count: seen.count, error: last_error(&seen) })
        .collect()
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(bytes) => String::from_utf8_lossy(&bytes.iter().filter_map(|byte| byte.as_u64().map(|byte| byte as u8)).collect::<Vec<u8>>()).into_owned(),
        _ => String::new(),
    }
}

fn last_error(seen: &Seen) -> String {
    let errorish = seen.lines.iter().rev().find(|line| {
        let lower = line.to_lowercase();
        ERROR_WORDS.iter().any(|word| lower.contains(word))
    });
    errorish.or(seen.lines.last()).unwrap_or(&seen.start_message).clone()
}

pub(super) fn verdict(failing: &[Failing]) -> Result<(), String> {
    if failing.is_empty() {
        return Ok(());
    }
    Err(failing.iter().map(|one| format!("{} failed to start {} times in the last hour: {}", one.unit, one.count, one.error)).collect::<Vec<_>>().join("; "))
}

fn key(failing: &Failing) -> String {
    let shape: String = failing.error.chars().fold(String::new(), |mut shape, character| {
        if !(character.is_ascii_digit() && shape.ends_with('#')) {
            shape.push(if character.is_ascii_digit() { '#' } else { character });
        }
        shape
    });
    format!("{}\n{shape}", failing.unit)
}

pub(super) fn announce(failing: &[Failing], now: u64, told: &mut Told, send: &mut dyn FnMut(&str) -> bool) {
    let day = now / DAY;
    if told.day != day {
        told.day = day;
        told.sent.clear();
    }
    for one in failing {
        let key = key(one);
        if told.sent.contains(&key) {
            continue;
        }
        let message = format!("harness unit {} failed to start {} times in the last hour: {}", one.unit, one.count, one.error);
        if send(&message) {
            told.sent.push(key);
        }
    }
}

pub(super) fn journal() -> Result<String, String> {
    super::command(
        "journalctl",
        &["--user", "--since", "-1h", "-o", "json", "--output-fields=MESSAGE,USER_UNIT,_SYSTEMD_USER_UNIT,SYSLOG_IDENTIFIER", "--no-pager"],
        None,
        20,
    )
}

pub(super) fn tell_proxy(message: &str) -> bool {
    super::command("north", &["fleet", "send", "proxy", message], None, 20).is_ok()
}

pub(super) fn check(dry_run: bool, now: u64, state_file: &std::path::Path, read: &dyn Fn() -> Result<String, String>, send: &mut dyn FnMut(&str) -> bool) -> Result<(), String> {
    let failing = scan(&read()?);
    if !dry_run && !failing.is_empty() {
        let mut told: Told = std::fs::read(state_file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        announce(&failing, now, &mut told, send);
        super::save(state_file, &told)?;
    }
    verdict(&failing)
}
