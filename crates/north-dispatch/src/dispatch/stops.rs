//! What the last dispatched worker's run asks of the next launch, read from records rather than its report: a box whose
//! session opened a gate for Tom is held by that open gate (`survey::gate_rows`), a dated box by its `(not before ...)`
//! stamp; after a worker recorded a need with `north fleet need` the next brief carries `Needs: NEED`, and every later one while a box
//! it named stays unticked (`host_needs`), so placement sends it to a host that serves it instead of the one that could not. When the run shows a spin signal (`spin`), the next launch
//! climbs one rung of `Rung::LADDER` (Opus xhigh twice, then Opus max once), and from its top rung the box waits for a decision.
use super::issues::{Issue, IssueRef};
use super::record::Record;
use super::spin::{self, Evidence, Signal};
use super::work::did_work;
use super::tier::Rung;
use crate::fleet::records::RetiredReport;
use crate::platform::land_notice::Notice;
use serde_json::Value;
use std::collections::BTreeMap;

/// The holds a run puts on its box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stop {
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

/// The entries of a retired session that dispatch reads, as `(kind, text)` in order: its first and last assistant message,
/// every lander notice it was sent (`spin`) and its first commit or check run (`tries::unspent`), so a host that mirrors
/// the session sends a few messages and not its whole transcript.
pub fn entries_read(entries: &[(String, String)]) -> Vec<(String, String)> {
    let said: Vec<usize> = (0..entries.len()).filter(|&at| entries[at].0 == "assistant").collect();
    let worked = (0..entries.len()).find(|&at| entries[at].0 == "tool" && did_work(&entries[at].1));
    let mut kept: std::collections::BTreeSet<usize> = said.first().into_iter().chain(said.last()).copied().chain(worked).chain((0..entries.len()).filter(|&at| matches!(entries[at].0.as_str(), "user" | "notice") && Notice::delivered(&entries[at].1).is_some())).collect();
    kept.extend((0..entries.len()).filter(|&at| entries[at].0 == "refusal"));
    kept.into_iter().map(|at| entries[at].clone()).collect()
}

/// The session's newest assistant message, which a climb carries into the next brief to start from.
pub(super) fn final_report(session: &Value) -> Option<&str> {
    session["entries"].as_array()?.iter().rev().filter(|entry| entry["kind"] == "assistant").find_map(|entry| entry["text"].as_str())
}

/// The holds of `launch`'s session, from `north fleet show NAME --json`.
pub fn stop(session: &Value, launch: &Record, evidence: &Evidence) -> Stop {
    let Some(report) = final_report(session) else {
        return Stop::default();
    };
    let ended = session["turns"].as_array().and_then(|turns| turns.last()).and_then(|turn| turn["ended_ms"].as_f64()).map(|ms| ms / 1000.0);
    holds(report, ended, spin::observed(session, evidence), launch)
}

/// The holds of a retired session of `launch`, whose final report the fleet's archive keeps after the reaper retires it:
/// the climb reads only the ledger's evidence, since its lander notices went with the session.
pub fn retired_stop(retired: &RetiredReport, launch: &Record, evidence: &Evidence) -> Stop {
    let ended = Some(retired.at_ms as f64 / 1000.0);
    holds(&retired.text, ended, spin::judged(evidence), launch)
}

fn holds(report: &str, ended: Option<f64>, spin: Option<spin::Spin>, launch: &Record) -> Stop {
    let climb = spin.and_then(|spin| Some(Climb { session: launch.name.clone(), rung: Rung::recorded(launch)?, ended: ended?, report: report.trim().to_owned(), signal: spin.signal, why: spin.evidence }));
    Stop { relay: None, climb }
}

/// Whether the issue is unchanged since its worker on the ladder's top rung got nowhere.
pub fn awaits_decision(issue: &Issue) -> bool {
    issue.stop.climb.as_ref().filter(|climb| climb.rung.above().is_none()).is_some_and(|climb| issue.touched <= climb.ended)
}
