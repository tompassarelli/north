use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::fleet::model::{Effort, Ending, SessionRecord, Status};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::SessionRow;
use crate::fleet::waits::Waits;

const USAGE: &str = "Usage: north fleet list [--json [--all]] | north fleet stranded --json";
const STALE_MINUTES: u64 = 5;
const MISSING_MINUTES: u64 = 7 * 24 * 60;

pub(super) fn list(fleet: &Fleet, verb: &str, words: &[&str]) -> NorthResult<()> {
    match (verb, words) {
        ("list", []) => list_lines(fleet),
        ("list", ["--json"]) => list_json(fleet, false),
        ("list", ["--json", "--all"] | ["--all", "--json"]) => list_json(fleet, true),
        ("stranded", ["--json"]) => stranded(fleet),
        ("rebooted", flags) => super::rebooted::run(fleet, flags),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

/// The stopped sessions holding unlanded lanes, with each lane's standing.
fn stranded(fleet: &Fleet) -> NorthResult<()> {
    println!("{}", serde_json::to_value(fleet.stranded(&crate::fleet::lane_probe::Git)?)?);
    Ok(())
}

fn list_lines(fleet: &Fleet) -> NorthResult<()> {
    let records = fleet.sessions()?;
    for record in &records {
        println!("{}", list_line(record, fleet.status(record)));
    }
    let now = now_ms();
    let named = records.iter().map(|record| record.name.clone()).collect();
    for row in elsewhere(fleet, &named) {
        println!("{}", remote_line(&row, now));
    }
    Ok(())
}

fn list_line(record: &SessionRecord, status: Status) -> String {
    let cwd = match &record.machine {
        Some(machine) => format!("{}:{}", machine.name, record.cwd.display()),
        None => record.cwd.display().to_string(),
    };
    columns(&record.name, record.provider.label(), &record.account, status.label(), &cwd, record.parent.as_deref(), record.placement.as_deref())
}

/// The fleet table's sessions that run on another host, as this host's last tick cached it: this host's own rows give way to its live records, and the table never makes the list wait on the ledger host.
pub(super) fn elsewhere(fleet: &Fleet, named: &BTreeSet<String>) -> Vec<SessionRow> {
    let rows = match fleet.cached_sessions() {
        Ok(rows) => rows.unwrap_or_default(),
        Err(error) => {
            eprintln!("north fleet: the cached fleet table is unreadable, so only this host's sessions are listed: {}", error.user_message());
            Vec::new()
        }
    };
    let now = now_ms();
    rows.into_iter().filter(|row| row.host != fleet.host() && !named.contains(&row.name) && minutes_since(row, now) < MISSING_MINUTES).collect()
}

fn updated_ms(row: &SessionRow) -> u64 {
    Timestamp::parse(&row.updated).ok().and_then(|at| u64::try_from(at.unix_millis()).ok()).unwrap_or(0)
}

fn minutes_since(row: &SessionRow, now: u64) -> u64 {
    now.saturating_sub(updated_ms(row)) / 60_000
}

fn remote_line(row: &SessionRow, now: u64) -> String {
    let minutes = minutes_since(row, now);
    let status = if minutes >= STALE_MINUTES { format!("{} (last seen {minutes}m ago)", row.status) } else { row.status.clone() };
    let cwd = match &row.machine {
        Some(machine) => format!("{}:{machine}:{}", row.host, row.cwd),
        None => format!("{}:{}", row.host, row.cwd),
    };
    columns(&row.name, &row.provider, &row.account, &status, &cwd, row.parent.as_deref(), row.placement.as_deref())
}

fn columns(name: &str, provider: &str, account: &str, status: &str, cwd: &str, parent: Option<&str>, placement: Option<&str>) -> String {
    let line = format!("{name}\t{provider}\t{account}\t{status}\t{cwd}\t{}", parent.unwrap_or("-"));
    match placement {
        Some(note) => format!("{line}\t{note}"),
        None => line,
    }
}

fn list_json(fleet: &Fleet, all: bool) -> NorthResult<()> {
    println!("{}", listing(fleet, all)?);
    Ok(())
}

/// This host's sessions; `all` adds its past sessions and those the fleet table holds for other hosts, a session once.
fn listing(fleet: &Fleet, all: bool) -> NorthResult<Value> {
    let waits = fleet.waits();
    let records = fleet.sessions()?;
    let mut sessions: Vec<Value> = records.iter().map(|record| session_json(record, fleet.status(record), &waits)).collect();
    if all {
        let named: BTreeSet<String> = records.iter().map(|record| record.name.clone()).collect();
        for record in fleet.past_sessions() {
            let mut session = session_json(&record, Status::Exited, &waits);
            session["past"] = json!(true);
            sessions.push(session);
        }
        let now = now_ms();
        for row in elsewhere(fleet, &named) {
            sessions.push(remote_json(&row, now));
        }
    }
    Ok(Value::Array(sessions))
}

fn remote_json(row: &SessionRow, now: u64) -> Value {
    json!({
        "name": row.name,
        "provider": row.provider,
        "account": row.account,
        "status": row.status,
        "parent": row.parent,
        "cwd": row.cwd,
        "task_ref": row.tasks.first(),
        "tasks": row.tasks,
        "placement": row.placement,
        "machine": row.machine,
        "conversation": row.conversation,
        "host": row.host,
        "remote": true,
        "seen_ms": updated_ms(row),
        "stale": minutes_since(row, now) >= STALE_MINUTES,
    })
}

pub(super) fn session_json(record: &SessionRecord, status: Status, waits: &Waits) -> Value {
    json!({
        "name": record.name,
        "provider": record.provider.label(),
        "account": record.account,
        "status": status.label(),
        "parent": record.parent,
        "role": record.role().label(),
        "cwd": record.cwd,
        "model": record.model,
        "effort": record.effort.map(Effort::label),
        "conversation": record.conversation,
        "task_ref": record.task_ref,
        "tasks": record.tasks().collect::<Vec<_>>(),
        "created_ms": record.created_ms,
        "host_pid": record.host_pid,
        "ended": record.ended.map(Ending::label),
        "placement": record.placement,
        "machine": record.machine.as_ref().map(|machine| &machine.name),
        "waiting": waits.of(&record.name).iter().map(|wait| wait.run.to_string()).collect::<Vec<_>>(),
    })
}
