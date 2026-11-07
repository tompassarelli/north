//! What the last dispatched worker's final report asks of the next launch: after `Stop: decision` or `Blocked:` the box
//! stays skipped until the issue gets a comment or body edit newer than that report; after `Stop: wait until TIME` it
//! stays skipped until that UTC time; after `Stop: host NEED` the next brief carries `Needs: NEED`, so placement sends it
//! to a host that serves it instead of the one that could not. After `Not done:` with `Stop: reasoning`, or after the
//! lander refused one of the session's lanes twice for its own failing checks without fewer errors, the next launch
//! climbs one rung of `Rung::LADDER`, and from its top rung the box waits for a decision like `Stop: decision`.
use super::issues::{Issue, IssueRef};
use super::record::Record;
use super::tier::Rung;
use crate::fleet::records::RetiredReport;
use crate::platform::land_notice::Notice;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*(?:Stop:\s*decision\b|Blocked:)").expect("stop pattern"));
static WAIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*wait\s+until\s+(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}Z)").expect("wait pattern"));
static HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*host\s+([a-z][\w.-]*)").expect("host pattern"));
static REASONING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*reasoning\b").expect("reasoning pattern"));
static NOT_DONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\W*Not done:").expect("not done pattern"));

/// The holds a final report puts on its box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stop {
    /// When the report named `Stop: decision` or opened `Blocked:`.
    pub decision: Option<f64>,
    /// `Stop: wait until TIME`: the time as written and as unix seconds.
    pub until: Option<(String, f64)>,
    /// `Stop: host NEED`: the host or local need the next launch must be placed on.
    pub host: Option<String>,
    /// The worker got nowhere on its rung, so the next launch climbs one.
    pub climb: Option<Climb>,
}

/// A launch whose box the next launch climbs one rung from, with the evidence that it got nowhere on its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Climb {
    pub session: String,
    pub rung: Rung,
    pub ended: f64,
    pub report: String,
    pub why: String,
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

pub fn host_need(report: &str) -> Option<String> {
    HOST.captures(report).map(|found| found[1].to_lowercase())
}

/// Whether a final report is `Not done:` with `Stop: reasoning`: the worker ran out of approach on its rung.
fn out_of_approach(report: &str) -> bool {
    NOT_DONE.is_match(report) && REASONING.is_match(report)
}

fn final_report(session: &Value) -> Option<&str> {
    session["entries"].as_array()?.iter().rfind(|entry| entry["kind"] == "assistant")?["text"].as_str()
}

/// Why the session in `north fleet show NAME --json` got nowhere on its rung: its final report is `Not done:` with
/// `Stop: reasoning`, or the lander refused one of its lanes twice running for the lane's own failing checks, the
/// second time with as many errors or more, and nothing of the session landed after.
pub fn nowhere(session: &Value) -> Option<String> {
    if final_report(session).is_some_and(out_of_approach) {
        return Some("Not done with Stop: reasoning".into());
    }
    let delivered = session["entries"].as_array()?.iter().filter(|entry| entry["kind"] == "user" || entry["kind"] == "notice").filter_map(|entry| Notice::delivered(entry["text"].as_str()?));
    let mut tries: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for notice in delivered {
        match notice {
            Notice::Landed { .. } => tries.clear(),
            Notice::Refused { branch, errors: Some(errors), .. } => tries.entry(branch).or_default().push(errors),
            Notice::Refused { errors: None, .. } => {}
        }
    }
    tries.into_iter().find_map(|(lane, counts)| match counts[..] {
        [.., before, after] if after >= before => Some(format!("lane {lane}, refused twice by its own landing checks without fewer errors ({before}, then {after})")),
        _ => None,
    })
}

/// The holds of the final report of `launch`'s session, from `north fleet show NAME --json`.
pub fn stop(session: &Value, launch: &Record) -> Stop {
    let Some(report) = final_report(session) else {
        return Stop::default();
    };
    let ended = session["turns"].as_array().and_then(|turns| turns.last()).and_then(|turn| turn["ended_ms"].as_f64()).map(|ms| ms / 1000.0);
    holds(report, ended, nowhere(session), launch)
}

/// The holds of the final report a retired session of `launch` left in the fleet's archive, which outlives the session
/// the reaper retires once its report is final: the climb reads only the report, since its lander notices went with it.
pub fn retired_stop(retired: &RetiredReport, launch: &Record) -> Stop {
    let ended = Some(retired.at_ms as f64 / 1000.0);
    holds(&retired.text, ended, out_of_approach(&retired.text).then(|| "Not done with Stop: reasoning".to_owned()), launch)
}

fn holds(report: &str, ended: Option<f64>, nowhere: Option<String>, launch: &Record) -> Stop {
    let until = WAIT.captures(report).and_then(|found| {
        let written = found.get(1)?.as_str();
        Some((written.to_owned(), crate::foundation::clock::Timestamp::parse_loose(written)?.unix_seconds_f64()))
    });
    let climb = nowhere.and_then(|why| Some(Climb { session: launch.name.clone(), rung: Rung::recorded(launch)?, ended: ended?, report: report.trim().to_owned(), why }));
    Stop { decision: ended.filter(|_| STOP.is_match(report)), until, host: host_need(report), climb }
}

/// Whether the issue is unchanged since the report that stopped for a decision, or since its worker on the ladder's
/// top rung got nowhere.
pub fn awaits_decision(issue: &Issue) -> bool {
    let topped = issue.stop.climb.as_ref().filter(|climb| climb.rung.above().is_none()).map(|climb| climb.ended);
    issue.stop.decision.or(topped).is_some_and(|stopped| issue.touched <= stopped)
}

/// The time the last report waits until, while it is still ahead of `at`.
pub fn waits_until(issue: &Issue, at: f64) -> Option<&str> {
    issue.stop.until.as_ref().filter(|(_, until)| *until > at).map(|(written, _)| written.as_str())
}
