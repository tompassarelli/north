//! The spend breaker's part of `north fleet tick`: the fleet's workers become `Spending`, `usage::breaker::judge` decides,
//! and this applies it: events to `usage/breaker.jsonl`, the next tick's state to `usage/breaker.json`, tells to sessions,
//! alerts to Tom.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::fleet::gates::Answerer;
use crate::fleet::model::{Delivery, Role, SessionRecord};
use crate::fleet::registry::Fleet;
use crate::fleet::sender::Sender;
use crate::usage::breaker::{Answer, Estimate, Event, Spending, State, judge};
use crate::watch;

const STATE: &str = "agents/usage/breaker.json";
const LEDGER: &str = "agents/usage/breaker.jsonl";
const ANSWER_WINDOW_MS: u64 = 7 * 24 * 3_600_000;

pub(super) fn watch(fleet: &Fleet, tell_tom: &dyn Fn(&str) -> Result<(), String>, now_ms: u64) -> Vec<String> {
    let home = watch::state_home();
    let (state_path, ledger_path) = (home.join(STATE), home.join(LEDGER));
    let previous = match State::read(&state_path) {
        Ok(state) => state,
        Err(why) => return vec![format!("the spend breaker's state is unreadable, so it did not run: {why}")],
    };
    let sessions = spendings(fleet, now_ms);
    let judged = judge(now_ms, &sessions, &answers(fleet, now_ms), &previous);
    let mut lines = Vec::new();
    if let Err(why) = record(&ledger_path, &judged.events) {
        lines.push(format!("the spend breaker's events were not recorded, so the next tick finds the same change and records it again: {why}"));
        return lines;
    }
    if let Err(why) = judged.state.write(&state_path) {
        lines.push(format!("the spend breaker's state was not kept, so the next tick judges from the last one: {why}"));
    }
    for tell in &judged.tells {
        let sent = fleet.send_signed(&Sender::fleet(), &tell.to, &tell.text, Delivery::Queued);
        lines.push(sent.map_or_else(|error| format!("{}: the budget notice was not delivered: {}", tell.to, error.user_message()), |()| format!("{}: told its budget", tell.to)));
    }
    for alert in &judged.alerts {
        lines.push(tell_tom(alert).map_or_else(|why| format!("{alert} (not sent to Tom: {why})"), |()| alert.clone()));
    }
    lines
}

fn record(path: &Path, events: &[Event]) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    for event in events {
        let line = serde_json::to_string(event).map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

fn spendings(fleet: &Fleet, now_ms: u64) -> Vec<Spending> {
    let live: Vec<SessionRecord> = fleet.sessions().unwrap_or_default().into_iter().filter(|record| record.provider.label() == "claude" && !record.verification && fleet.running(record)).collect();
    let boxes: BTreeSet<String> = live.iter().filter_map(box_of).collect();
    let live_names: BTreeSet<String> = live.iter().map(|record| record.name.clone()).collect();
    let earlier = fleet.past_sessions().into_iter().filter(|record| !live_names.contains(&record.name) && box_of(record).is_some_and(|named| boxes.contains(&named)));
    let leads: BTreeSet<String> = live.iter().filter_map(|record| record.parent.clone()).collect();
    live.iter().map(|record| spending(fleet, record, true, &leads, now_ms)).chain(earlier.map(|record| spending(fleet, &record, false, &leads, now_ms))).flatten().collect()
}

fn spending(fleet: &Fleet, record: &SessionRecord, live: bool, leads: &BTreeSet<String>, now_ms: u64) -> Option<Spending> {
    let role = record.role();
    let workstream = record.task_ref.clone()?;
    Some(Spending {
        name: record.name.clone(),
        parent: record.parent.clone(),
        box_id: box_of(record)?,
        workstream,
        lead: role == Role::Lead || leads.contains(&record.name),
        tom: role == Role::Proxy,
        live,
        spent: fleet.spend_of(&record.name).ok().flatten().and_then(|spent| spent.api_usd),
        minutes: if live { now_ms.saturating_sub(record.created_ms) as f64 / 60_000.0 } else { 0.0 },
        created_ms: record.created_ms,
        estimate: record.budget.as_ref().map(|stated| Estimate { p50: stated.p50(), p90: stated.p90(), minutes: stated.minutes as f64 }),
    })
}

/// A session's box: the one its brief names, else the session alone, since sessions of one issue often share a
/// checkout and a bare `Item:` line tells their boxes apart by nothing else.
fn box_of(record: &SessionRecord) -> Option<String> {
    let workstream = record.task_ref.as_deref()?;
    Some(record.box_name.clone().unwrap_or_else(|| format!("{workstream}@{}", record.name)))
}

fn answers(fleet: &Fleet, now_ms: u64) -> Vec<Answer> {
    let Ok(digest) = fleet.digest(now_ms.saturating_sub(ANSWER_WINDOW_MS)) else { return Vec::new() };
    digest
        .gates
        .into_iter()
        .filter_map(|gate| {
            let closure = gate.closure?;
            let by = match closure.by {
                Answerer::Tom => "tom".to_owned(),
                Answerer::Session(name) => name,
                Answerer::Default => return None,
            };
            Some(Answer { workstream: gate.request.issue?, at_ms: closure.closed_ms, by, text: closure.answer })
        })
        .collect()
}
