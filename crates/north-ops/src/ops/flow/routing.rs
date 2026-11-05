//! Tier routing that silently alternates: an armed category whose Sonnet and Opus arms both hold
//! `ARM_RUNS` stamped runs routes on cost per closed box, so if it still alternates the ledger's
//! closed-box report or arm count was unread or unpriced, and nothing else would say so.
use std::collections::BTreeMap;
use std::path::Path;

use super::alerts::{Action, Node, Record, State};
use super::host::{Host, Places};
use crate::ledger::ClosedReport;
use crate::ops::dispatch::{ARMED, ARM_WINDOW, Evidence, Tier};

const STATE_FILE: &str = "routing.json";
const KEY: &str = "routing:supply";

/// The categories that keep alternating with full arms, each with the unread input.
pub fn stalls(database: &Path, routes: &BTreeMap<String, Tier>, closed: impl FnOnce() -> Result<ClosedReport, String>) -> Vec<(String, String)> {
    let full: Vec<(&str, Evidence)> = ARMED
        .iter()
        .filter_map(|category| {
            let stamped = crate::ledger::arm_runs_in(database, category).ok()?;
            let evidence = Evidence::default().counted(&stamped);
            Some((*category, evidence)).filter(|(category, evidence)| evidence.arms_full() && !routes.contains_key(*category))
        })
        .collect();
    if full.is_empty() {
        return Vec::new();
    }
    let report = closed();
    full.into_iter()
        .filter_map(|(category, evidence)| {
            let why = match &report {
                Err(why) => format!("north ledger summary --since {ARM_WINDOW} is unread: {why}"),
                Ok(report) => {
                    let priced = evidence.priced(report, category);
                    format!("no closed {category} box in the last {ARM_WINDOW} prices {}", priced.unpriced_arm()?.name())
                }
            };
            Some((category.to_owned(), why))
        })
        .collect()
}

/// One normal supply-alert per episode of a stalled category, once two runs in a row see it; a
/// category that routes again drops its record, so its next stall is a new episode.
pub fn routing_plan(stalled: &[(String, String)], previous: &State, now: f64) -> (State, Vec<Action>) {
    let mut state: State = previous.iter().filter(|(category, _)| stalled.iter().any(|(stalled, _)| stalled == *category)).map(|(category, record)| (category.clone(), record.clone())).collect();
    let mut actions = Vec::new();
    for (category, why) in stalled {
        let record = state.entry(category.clone()).or_insert_with(|| Record { since: now, ..Record::default() });
        if record.breaches.record(KEY, true) && !record.sent.contains_key(KEY) {
            let text = format!("Tier routing of {category} still alternates its arms with {} arm runs on each: {why}", crate::router::policy::ARM_RUNS);
            actions.push(Action { repo: category.clone(), key: KEY.into(), node: String::new(), channel: super::alerts::SUPPLY, text });
        }
    }
    (state, actions)
}

pub fn routing_alert(places: &Places, host: &dyn Host, now: f64) -> Result<(), String> {
    alert(places, host, &crate::ops::dispatch::tuned_routes(), || crate::ledger::closed_report(ARM_WINDOW).map_err(|error| error.user_message()), now)
}

pub(super) fn alert(places: &Places, host: &dyn Host, routes: &BTreeMap<String, Tier>, closed: impl FnOnce() -> Result<ClosedReport, String>, now: f64) -> Result<(), String> {
    let stalled = stalls(&places.ledger, routes, closed);
    super::alerts::deliver(places, host, STATE_FILE, |previous, _: &[Node]| routing_plan(&stalled, previous, now), now)
}
