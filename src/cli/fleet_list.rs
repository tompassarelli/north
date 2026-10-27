use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord, Status};
use crate::fleet::registry::Fleet;
use crate::fleet::remote_view::{Seen, SessionView};
use crate::fleet::runtime::now_ms;

const USAGE: &str = "Usage: north fleet list [--json [--all]]";
const STALE_MINUTES: u64 = 5;

pub(super) fn list(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        [] => list_lines(fleet),
        ["--json"] => list_json(fleet, false),
        ["--json", "--all"] => list_json(fleet, true),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

fn list_lines(fleet: &Fleet) -> NorthResult<()> {
    for record in fleet.sessions()? {
        println!("{}", list_line(&record, fleet.status(&record)));
    }
    let now = now_ms();
    for seen in fleet.remote_views() {
        for session in &seen.snapshot.sessions {
            println!("{}", remote_line(&seen, session, now));
        }
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

fn remote_line(seen: &Seen, session: &SessionView, now: u64) -> String {
    let minutes = now.saturating_sub(seen.at_ms) / 60_000;
    let status = if minutes >= STALE_MINUTES { format!("{} (last seen {minutes}m ago)", session.status) } else { session.status.clone() };
    let cwd = format!("{}:{}", seen.snapshot.host, session.cwd);
    columns(&session.name, &session.provider, &session.account, &status, &cwd, session.parent.as_deref(), session.placement.as_deref())
}

fn columns(name: &str, provider: &str, account: &str, status: &str, cwd: &str, parent: Option<&str>, placement: Option<&str>) -> String {
    let line = format!("{name}\t{provider}\t{account}\t{status}\t{cwd}\t{}", parent.unwrap_or("-"));
    match placement {
        Some(note) => format!("{line}\t{note}"),
        None => line,
    }
}

fn list_json(fleet: &Fleet, all: bool) -> NorthResult<()> {
    let mut sessions: Vec<Value> = fleet.sessions()?.iter().map(|record| session_json(record, fleet.status(record))).collect();
    if all {
        for record in fleet.past_sessions() {
            let mut session = session_json(&record, Status::Exited);
            session["past"] = json!(true);
            sessions.push(session);
        }
    }
    println!("{}", Value::Array(sessions));
    Ok(())
}

pub(super) fn session_json(record: &SessionRecord, status: Status) -> Value {
    json!({
        "name": record.name,
        "provider": record.provider.label(),
        "account": record.account,
        "status": status.label(),
        "parent": record.parent,
        "cwd": record.cwd,
        "model": record.model,
        "effort": record.effort.map(Effort::label),
        "conversation": record.conversation,
        "task_ref": record.task_ref,
        "created_ms": record.created_ms,
        "host_pid": record.host_pid,
        "placement": record.placement,
        "machine": record.machine.as_ref().map(|machine| &machine.name),
    })
}
