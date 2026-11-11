//! Budget notices in `north fleet tick`: the fleet's workers become `Spending`, `usage::budget::judge` decides, and this
//! applies it: events to `usage/budget.jsonl`, the next tick's state to `usage/budget.json`, tells to sessions, alerts
//! to Tom.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use crate::fleet::landings::{self, Push};
use crate::fleet::model::{Delivery, Role, SessionRecord};
use crate::fleet::registry::Fleet;
use crate::fleet::sender::Sender;
use crate::usage::budget::{Estimate, Event, Spending, State, judge};
use crate::watch;

const STATE: &str = "agents/usage/budget.json";
const LEDGER: &str = "agents/usage/budget.jsonl";

pub(super) fn watch(fleet: &Fleet, tell_tom: &dyn Fn(&str) -> Result<(), String>, now_ms: u64) -> Vec<String> {
    let home = watch::state_home();
    let (state_path, ledger_path) = (home.join(STATE), home.join(LEDGER));
    let previous = match State::read(&state_path) {
        Ok(state) => state,
        Err(why) => return vec![format!("the budget notices' state is unreadable, so none were judged: {why}")],
    };
    let sessions = spendings(fleet, now_ms);
    let judged = judge(now_ms, &sessions, &previous);
    let mut lines = Vec::new();
    if let Err(why) = record(&ledger_path, &judged.events) {
        lines.push(format!("the budget notices' events were not recorded, so the next tick finds the same change and records it again: {why}"));
        return lines;
    }
    if let Err(why) = judged.state.write(&state_path) {
        lines.push(format!("the budget notices' state was not kept, so the next tick judges from the last one: {why}"));
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
    let pushes = landings::pushes(&landings::push_log());
    let seen = |record: &SessionRecord, live: bool| spending(fleet, record, (live, &leads), &pushes, now_ms);
    live.iter().map(|record| seen(record, true)).chain(earlier.map(|record| seen(&record, false))).flatten().collect()
}

fn spending(fleet: &Fleet, record: &SessionRecord, (live, leads): (bool, &BTreeSet<String>), pushes: &[Push], now_ms: u64) -> Option<Spending> {
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
        estimate: record.budget.as_ref().map(|stated| Estimate { p50: stated.p50(), p90: stated.p90(), minutes: stated.minutes as f64 }),
        landed: landed_lanes(record, pushes, now_ms),
    })
}

/// A session's box: the one its brief names, else the session alone, since sessions of one issue often share a
/// checkout and a bare `Item:` line tells their boxes apart by nothing else.
fn box_of(record: &SessionRecord) -> Option<String> {
    let workstream = record.task_ref.as_deref()?;
    Some(record.box_name.clone().unwrap_or_else(|| format!("{workstream}@{}", record.name)))
}

fn landed_lanes(record: &SessionRecord, pushes: &[Push], now_ms: u64) -> u32 {
    let Some(repository) = record.task_ref.as_deref().and_then(|task| task.split_once('#')).map(|(repository, _)| repository.rsplit('/').next().unwrap_or(repository)) else { return 0 };
    let lanes: BTreeSet<&str> = landings::landings_by(pushes, &record.name, record.created_ms, now_ms).filter(|push| push.repository == repository).map(|push| push.branch.as_str()).collect();
    u32::try_from(lanes.len()).unwrap_or(u32::MAX)
}
