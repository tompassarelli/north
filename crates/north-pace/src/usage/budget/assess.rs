use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::Spending;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Box(String),
    Workstream(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Assessment {
    Unestimated,
    Warn,
    Velocity,
    Within,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spend {
    pub raw: Assessment,
    pub confirmed: Assessment,
    pub changed: bool,
    pub spent: f64,
    pub p50: f64,
    pub p90: f64,
}

#[derive(Debug, Clone, Default)]
struct Totals {
    spent: f64,
    p50: f64,
    p90: f64,
    minutes: f64,
    active_minutes: f64,
    estimated: bool,
}

pub fn spend(sessions: &[Spending], previous: &BTreeMap<Scope, Spend>) -> BTreeMap<Scope, Spend> {
    let counted: Vec<&Spending> = sessions.iter().filter(|session| !session.tom).collect();
    let boxes = box_totals(&counted);
    let mut answers = BTreeMap::new();
    for (id, (_, totals)) in &boxes {
        let scope = Scope::Box((*id).to_owned());
        let judged = judge(totals, previous.get(&scope));
        answers.insert(scope, judged);
    }
    let workstreams: BTreeSet<&str> = counted.iter().map(|session| session.workstream.as_str()).collect();
    for workstream in workstreams {
        let mut sum = Totals::default();
        for (_, totals) in boxes.values().filter(|(owner, _)| *owner == workstream) {
            sum = add(sum, totals);
        }
        let scope = Scope::Workstream(workstream.to_owned());
        let judged = judge(&sum, previous.get(&scope));
        answers.insert(scope, judged);
    }
    answers
}

fn box_totals<'a>(sessions: &[&'a Spending]) -> BTreeMap<&'a str, (&'a str, Totals)> {
    let mut boxes: BTreeMap<&str, (&str, Totals)> = BTreeMap::new();
    for session in sessions.iter().filter(|session| !session.lead) {
        let (workstream, totals) = boxes.entry(session.box_id.as_str()).or_insert((session.workstream.as_str(), Totals::default()));
        *workstream = (*workstream).min(session.workstream.as_str());
        totals.spent += session.spent.unwrap_or(0.0);
        totals.active_minutes += session.minutes;
        if let Some(expected) = session.expected() {
            totals.p50 = totals.p50.max(expected.p50.unwrap_or(expected.p90));
            totals.p90 = totals.p90.max(expected.p90);
            totals.minutes = totals.minutes.max(expected.minutes);
            totals.estimated = true;
        }
    }
    boxes
}

fn add(left: Totals, right: &Totals) -> Totals {
    Totals {
        spent: left.spent + right.spent,
        p50: left.p50 + right.p50,
        p90: left.p90 + right.p90,
        minutes: left.minutes + right.minutes,
        active_minutes: left.active_minutes + right.active_minutes,
        estimated: left.estimated || right.estimated,
    }
}

fn judge(totals: &Totals, previous: Option<&Spend>) -> Spend {
    let raw = classify(totals);
    let confirmed = match previous {
        _ if raw == Assessment::Unestimated => raw,
        Some(before) if before.raw == raw => raw,
        Some(before) => before.confirmed,
        None => Assessment::Within,
    };
    let changed = previous.map_or(confirmed != Assessment::Within, |before| before.confirmed != confirmed);
    Spend { raw, confirmed, changed, spent: totals.spent, p50: totals.p50, p90: totals.p90 }
}

fn classify(totals: &Totals) -> Assessment {
    if !totals.estimated || totals.p90 <= 0.0 {
        Assessment::Unestimated
    } else if totals.spent >= 2.0 * totals.p90 {
        Assessment::Warn
    } else if totals.spent >= 0.5 * totals.p50 && totals.active_minutes < 0.25 * totals.minutes {
        Assessment::Velocity
    } else {
        Assessment::Within
    }
}
