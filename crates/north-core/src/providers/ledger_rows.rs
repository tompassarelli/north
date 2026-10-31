use std::collections::HashSet;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::providers::claude::runs::landing::MainIndex;
use crate::providers::claude::runs::telemetry::Prices;

pub const RUNNING_SECONDS: f64 = 7200.0;

static FIELDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:Item|Follows|Category|Arm|Spec|Scope|Surface|Verify|Lens):[^\n]+").unwrap());
static NOT_DONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*(not|partly|mostly|half) done").unwrap());
static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*split:").unwrap());
static BLOCKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*blocked").unwrap());
static DONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*done\b").unwrap());

pub struct Shared<'a> {
    pub session: &'a str,
    pub since: &'a str,
    pub known: &'a HashSet<String>,
    pub recorded: &'a HashSet<String>,
    pub main: &'a MainIndex,
    pub prices: &'a Prices,
}

pub fn now() -> f64 {
    Timestamp::now().unix_seconds_f64()
}

pub fn cutoff(since: &str) -> NorthResult<f64> {
    if since.is_empty() {
        return Ok(now() - 86400.0);
    }
    Timestamp::parse_loose(since).map(Timestamp::unix_seconds_f64).ok_or_else(|| NorthError::Usage(format!("unreadable --since {since:?}, expected a date such as 2026-10-08")))
}

pub fn stamp(text: &str) -> Option<f64> {
    Timestamp::parse(text).ok().or_else(|| Timestamp::parse_loose(text)).map(Timestamp::unix_seconds_f64)
}

pub fn outcome(report: &str) -> &'static str {
    let head: String = report.chars().take(400).collect();
    if head.contains("HANDOFF") {
        "handoff"
    } else if SPLIT.is_match(&head) {
        "split"
    } else if NOT_DONE.is_match(&head) {
        "not done"
    } else if BLOCKED.is_match(&head) {
        "escalated"
    } else if DONE.is_match(&head) {
        "done"
    } else {
        "unclear"
    }
}

pub fn fields(brief: &str) -> String {
    FIELDS.find_iter(brief).map(|found| found.as_str()).collect::<Vec<_>>().join("\n")
}

pub fn minutes(started: f64, ended: f64) -> i64 {
    (((ended - started) / 60.0).trunc() as i64).max(0)
}

pub fn status(final_message: bool, ended: f64) -> &'static str {
    if final_message {
        "finished"
    } else if now() - ended < RUNNING_SECONDS {
        "running"
    } else {
        "stale"
    }
}

pub fn fleet_nodes() -> Option<Vec<Value>> {
    let output = Command::new(crate::providers::claude::runs::north_program()).args(["fleet", "list", "--json"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

pub fn session_role(nodes: Option<&Vec<Value>>, session: &str) -> Option<&'static str> {
    let nodes = nodes?;
    let node = nodes.iter().find(|row| row["conversation"] == session)?;
    if crate::providers::claude::runs::truthy(&node["parent"]) {
        Some("worker")
    } else if nodes.iter().any(|row| row["parent"] == node["name"]) {
        Some("parent")
    } else {
        Some("proxy")
    }
}
