//! What the last dispatched worker's final report asks of the next launch: after `Stop: decision` or `Blocked:` the box
//! stays skipped until the issue gets a comment or body edit newer than that report; after `Stop: wait until TIME` it
//! stays skipped until that UTC time, and so does every later launch on a box that wait named (`wait_stops`); after a worker recorded a need with `north fleet need` the next brief carries `Needs: NEED`, and every later one while a box
//! it named stays unticked (`host_needs`), so placement sends it to a host that serves it instead of the one that could not. When the run shows a spin signal (`spin`), the next launch
//! climbs one rung of `Rung::LADDER` (Opus xhigh twice, then Opus max once), and from its top rung the box waits for a decision like `Stop: decision`.
use super::issues::{Issue, IssueRef};
use super::record::Record;
use super::spin::{self, Evidence, Signal};
use super::work::did_work;
use super::tier::Rung;
use crate::fleet::records::RetiredReport;
use crate::platform::land_notice::Notice;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*(?:Stop:\s*decision\b|Blocked:)").expect("stop pattern"));
static WAIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*wait\s+until\s+(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}Z)").expect("wait pattern"));
static REPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\W*(?:Done|Not done|Split|Blocked):").expect("report pattern"));

/// The holds a final report puts on its box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stop {
    /// When the report named `Stop: decision` or opened `Blocked:`.
    pub decision: Option<f64>,
    /// `Stop: wait until TIME`: the time as written and as unix seconds.
    pub until: Option<(String, f64)>,
    /// The stopped run the next launch follows when its session recorded a host need with `north fleet need`, with the reason that brief carries.
    pub relay: Option<Relay>,
    /// The worker got nowhere on its rung, so the next launch climbs one.
    pub climb: Option<Climb>,
}

/// A launch whose session recorded a host need: the relaunch on a host that serves the need follows it and starts from the reason recorded.
#[derive(Clone, Debug, PartialEq)]
pub struct Relay {
    pub session: String,
    pub need: String,
    pub report: String,
}

/// A launch whose box the next launch climbs one rung from, with the evidence that it got nowhere on its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Climb {
    pub session: String,
    pub rung: Rung,
    pub ended: f64,
    pub report: String,
    /// The signal that fired, which the launch the climb causes records.
    pub signal: Signal,
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

/// The entries of a retired session that dispatch reads, as `(kind, text)` in order: its first and last assistant message and
/// the newest one opening with a report word (`stop`, `unspent`), every lander notice it was sent (`spin`) and its first
/// commit or check run (`tries::unspent`), so a host that mirrors the session sends a few messages and not its whole transcript.
pub fn entries_read(entries: &[(String, String)]) -> Vec<(String, String)> {
    let said: Vec<usize> = (0..entries.len()).filter(|&at| entries[at].0 == "assistant").collect();
    let report = said.iter().rfind(|&&at| REPORT.is_match(&entries[at].1)).copied();
    let worked = (0..entries.len()).find(|&at| entries[at].0 == "tool" && did_work(&entries[at].1));
    let kept: std::collections::BTreeSet<usize> = said.first().into_iter().chain(said.last()).copied().chain(report).chain(worked).chain((0..entries.len()).filter(|&at| matches!(entries[at].0.as_str(), "user" | "notice") && Notice::delivered(&entries[at].1).is_some())).collect();
    kept.into_iter().map(|at| entries[at].clone()).collect()
}

/// The newest assistant message that opens with a report word, else the newest assistant message: a reply to a later
/// notice ("the lander's message confirms the landing I already reported") is not the report.
pub(super) fn final_report(session: &Value) -> Option<&str> {
    let said: Vec<&str> = session["entries"].as_array()?.iter().filter(|entry| entry["kind"] == "assistant").filter_map(|entry| entry["text"].as_str()).collect();
    said.iter().rfind(|text| REPORT.is_match(text)).or(said.last()).copied()
}

/// The holds of the final report of `launch`'s session, from `north fleet show NAME --json`.
pub fn stop(session: &Value, launch: &Record, evidence: &Evidence) -> Stop {
    let Some(report) = final_report(session) else {
        return Stop::default();
    };
    let ended = session["turns"].as_array().and_then(|turns| turns.last()).and_then(|turn| turn["ended_ms"].as_f64()).map(|ms| ms / 1000.0);
    holds(report, ended, spin::observed(session, evidence), launch)
}

/// The holds of the final report a retired session of `launch` left in the fleet's archive, which outlives the session
/// the reaper retires once its report is final: the climb reads only the report, since its lander notices went with it.
pub fn retired_stop(retired: &RetiredReport, launch: &Record, evidence: &Evidence) -> Stop {
    let ended = Some(retired.at_ms as f64 / 1000.0);
    holds(&retired.text, ended, spin::judged(&retired.text, evidence), launch)
}

fn holds(report: &str, ended: Option<f64>, spin: Option<spin::Spin>, launch: &Record) -> Stop {
    let until = WAIT.captures(report).and_then(|found| {
        let written = found.get(1)?.as_str();
        Some((written.to_owned(), crate::foundation::clock::Timestamp::parse_loose(written)?.unix_seconds_f64()))
    });
    let climb = spin.and_then(|spin| Some(Climb { session: launch.name.clone(), rung: Rung::recorded(launch)?, ended: ended?, report: report.trim().to_owned(), signal: spin.signal, why: spin.evidence }));
    Stop { decision: ended.filter(|_| STOP.is_match(report)), until, relay: None, climb }
}

/// Whether the issue is unchanged since the report that stopped for a decision, or since its worker on the ladder's
/// top rung got nowhere.
pub fn awaits_decision(issue: &Issue) -> bool {
    let topped = issue.stop.climb.as_ref().filter(|climb| climb.rung.above().is_none()).map(|climb| climb.ended);
    issue.stop.decision.or(topped).is_some_and(|stopped| issue.touched <= stopped)
}

/// The latest time ahead of `at` the last report, or a wait an earlier launch learned (`wait_stops`) for a box still open, waits until.
pub fn waits_until(issue: &Issue, at: f64) -> Option<&str> {
    issue.stop.until.iter().chain(&issue.learned_wait).filter(|(_, until)| *until > at).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(written, _)| written.as_str())
}
