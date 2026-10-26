use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord, Status};
use crate::fleet::registry::Fleet;

const USAGE: &str = "Usage: north fleet list [--json [--all]]";

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
    Ok(())
}

fn list_line(record: &SessionRecord, status: Status) -> String {
    let cwd = match &record.machine {
        Some(machine) => format!("{}:{}", machine.name, record.cwd.display()),
        None => record.cwd.display().to_string(),
    };
    let line = format!("{}\t{}\t{}\t{}\t{cwd}", record.name, record.provider.label(), record.account, status.label());
    match &record.placement {
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
