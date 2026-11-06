//! What the last dispatched worker's final report asks of the next launch: after `Stop: decision` or `Blocked:` the box
//! stays skipped until the issue gets a comment or body edit newer than that report; after `Stop: wait until TIME` it
//! stays skipped until that UTC time; after `Stop: host NEED` the next brief carries `Needs: NEED`, so placement sends it
//! to a host that serves it instead of the one that could not.
use super::issues::{Issue, IssueRef};
use super::record::Record;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*(?:Stop:\s*decision\b|Blocked:)").expect("stop pattern"));
static WAIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*wait\s+until\s+((\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})Z)").expect("wait pattern"));
static HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*host\s+([a-z][\w.-]*)").expect("host pattern"));

/// The holds a final report puts on its box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stop {
    /// When the report named `Stop: decision` or opened `Blocked:`.
    pub decision: Option<f64>,
    /// `Stop: wait until TIME`: the time as written and as unix seconds.
    pub until: Option<(String, f64)>,
    /// `Stop: host NEED`: the host or local need the next launch must be placed on.
    pub host: Option<String>,
}

/// Each item's newest launch that started a session, which holds the report dispatch last waited on.
pub fn last_launches(records: &[Record]) -> BTreeMap<IssueRef, &Record> {
    let mut newest: BTreeMap<IssueRef, &Record> = BTreeMap::new();
    for record in records.iter().filter(|record| record.launched && !record.cloud) {
        if let Some(item) = IssueRef::parse(&record.item)
            && newest.get(&item).is_none_or(|seen| seen.at <= record.at)
        {
            newest.insert(item, record);
        }
    }
    newest
}

/// The holds of the session's final report, from `north fleet show NAME --json`.
pub fn stop(session: &Value) -> Stop {
    let Some(report) = session["entries"].as_array().and_then(|entries| entries.iter().rfind(|entry| entry["kind"] == "assistant")).and_then(|entry| entry["text"].as_str()) else {
        return Stop::default();
    };
    let ended = session["turns"].as_array().and_then(|turns| turns.last()).and_then(|turn| turn["ended_ms"].as_f64()).map(|ms| ms / 1000.0);
    let until = WAIT.captures(report).and_then(|found| Some((found.get(1)?.as_str().to_owned(), super::issues::not_before(&found)? as f64)));
    Stop { decision: ended.filter(|_| STOP.is_match(report)), until, host: HOST.captures(report).map(|found| found[1].to_lowercase()) }
}

/// Whether the issue is unchanged since the report that stopped for a decision.
pub fn awaits_decision(issue: &Issue) -> bool {
    issue.stop.decision.is_some_and(|stopped| issue.touched <= stopped)
}

/// The time the last report waits until, while it is still ahead of `at`.
pub fn waits_until(issue: &Issue, at: f64) -> Option<&str> {
    issue.stop.until.as_ref().filter(|(_, until)| *until > at).map(|(written, _)| written.as_str())
}
