use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const LIMIT: usize = 3;
const ERROR_WORDS: [&str; 10] = ["error", "fail", "denied", "cannot", "unable", "broken", "panic", "fatal", "timed out", "timeout"];

#[derive(Debug, PartialEq)]
pub(super) struct Failing {
    pub(super) unit: String,
    pub(super) count: usize,
    pub(super) cause: String,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
pub(super) struct Told {
    #[serde(default)]
    units: BTreeSet<String>,
}

#[derive(Default)]
struct Seen {
    count: usize,
    timed_out: bool,
    result: Option<String>,
    output: Vec<String>,
}

impl Seen {
    fn started(&mut self) {
        *self = Self::default();
    }
}

pub(super) fn scan(journal: &str) -> Vec<Failing> {
    let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
    for entry in journal.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        let message = text(&entry["MESSAGE"]);
        if let Some(unit) = entry["USER_UNIT"].as_str() {
            let seen = seen.entry(unit.to_owned()).or_default();
            let own = message.strip_prefix(unit).and_then(|rest| rest.strip_prefix(": ")).unwrap_or("");
            if message.starts_with("Failed to start") {
                seen.count += 1;
            } else if message.starts_with("Started ") {
                seen.started();
            } else if own.starts_with("start operation timed out") {
                seen.timed_out = true;
            } else if own.starts_with("Failed with result") {
                seen.result = Some(own.trim_end_matches('.').to_owned());
            }
        } else if let Some(unit) = entry["_SYSTEMD_USER_UNIT"].as_str().filter(|_| entry["SYSLOG_IDENTIFIER"] != "systemd") {
            let line = message.trim();
            if !line.is_empty() {
                seen.entry(unit.to_owned()).or_default().output.push(line.to_owned());
            }
        }
    }
    seen.into_iter()
        .filter(|(_, seen)| seen.count >= LIMIT)
        .map(|(unit, seen)| Failing { unit, count: seen.count, cause: cause(&seen) })
        .collect()
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(bytes) => String::from_utf8_lossy(&bytes.iter().filter_map(|byte| byte.as_u64().map(|byte| byte as u8)).collect::<Vec<u8>>()).into_owned(),
        _ => String::new(),
    }
}

fn cause(seen: &Seen) -> String {
    if seen.timed_out {
        return "start operation timed out".into();
    }
    let errorish = seen.output.iter().find(|line| {
        let lower = line.to_lowercase();
        ERROR_WORDS.iter().any(|word| lower.contains(word))
    });
    errorish.cloned().or_else(|| seen.result.clone()).unwrap_or_else(|| "no cause logged".into())
}

pub(super) fn verdict(failing: &[Failing]) -> Result<(), String> {
    if failing.is_empty() {
        return Ok(());
    }
    Err(failing.iter().map(|one| format!("{} failed to start {} times in the last hour: {}", one.unit, one.count, one.cause)).collect::<Vec<_>>().join("; "))
}

fn condition(unit: &str) -> String {
    format!("harness-unit-start:{unit}")
}

/// Tells the proxy of each failing unit under its condition key on every check, which the fleet store delivers once per episode and again when the cause changes, and of each unit told failing that recovered, which ends the episode.
pub(super) fn announce(failing: &[Failing], told: &mut Told, send: &mut dyn FnMut(&str, &str, bool) -> bool) {
    for one in failing {
        if send(&format!("harness unit {} failed to start {} times in the last hour: {}", one.unit, one.count, one.cause), &condition(&one.unit), false) {
            told.units.insert(one.unit.clone());
        }
    }
    let recovered: Vec<String> = told.units.iter().filter(|unit| !failing.iter().any(|one| &one.unit == *unit)).cloned().collect();
    for unit in recovered {
        if send(&format!("harness unit {unit} recovered: under {LIMIT} failed starts in the last hour"), &condition(&unit), true) {
            told.units.remove(&unit);
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

pub(super) fn tell_proxy(message: &str, key: &str, cleared: bool) -> bool {
    let mut arguments = vec!["fleet", "send", "--from", "service:north-health", "--key", key];
    if cleared {
        arguments.push("--cleared");
    }
    arguments.extend(["proxy", message]);
    super::command("north", &arguments, None, 20).is_ok()
}

pub(super) fn check(dry_run: bool, state_file: &std::path::Path, read: &dyn Fn() -> Result<String, String>, send: &mut dyn FnMut(&str, &str, bool) -> bool) -> Result<(), String> {
    let failing = scan(&read()?);
    if !dry_run {
        let mut told: Told = std::fs::read(state_file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        if !failing.is_empty() || !told.units.is_empty() {
            announce(&failing, &mut told, send);
            super::save(state_file, &told)?;
        }
    }
    verdict(&failing)
}

#[cfg(test)]
#[path = "failed_starts_tests.rs"]
mod tests;
