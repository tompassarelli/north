use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, SessionRecord, Status};
use crate::fleet::registry::Fleet;
use crate::fleet::remote_view::{Seen, SessionView};
use crate::fleet::runtime::now_ms;
use crate::fleet::waits::Waits;

const USAGE: &str = "Usage: north fleet list [--json [--all]] | north fleet stranded --json";
const STALE_MINUTES: u64 = 5;

pub(super) fn list(fleet: &Fleet, verb: &str, words: &[&str]) -> NorthResult<()> {
    match (verb, words) {
        ("list", []) => list_lines(fleet),
        ("list", ["--json"]) => list_json(fleet, false),
        ("list", ["--json", "--all"] | ["--all", "--json"]) => list_json(fleet, true),
        ("stranded", ["--json"]) => stranded(fleet),
        ("rebooted", flags) => super::fleet_rebooted::run(fleet, flags),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

/// The stopped sessions holding unlanded lanes, with each lane's standing.
fn stranded(fleet: &Fleet) -> NorthResult<()> {
    println!("{}", serde_json::to_value(fleet.stranded(&crate::fleet::lane_probe::Git)?)?);
    Ok(())
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
    println!("{}", listing(fleet, all)?);
    Ok(())
}

/// This host's sessions; `all` adds its past sessions and those every relayed host reports, a session once.
fn listing(fleet: &Fleet, all: bool) -> NorthResult<Value> {
    let waits = fleet.waits();
    let mut sessions: Vec<Value> = fleet.sessions()?.iter().map(|record| session_json(record, fleet.status(record), &waits)).collect();
    if all {
        for record in fleet.past_sessions() {
            let mut session = session_json(&record, Status::Exited, &waits);
            session["past"] = json!(true);
            sessions.push(session);
        }
    }
    if all {
        let mut named: BTreeSet<String> = sessions.iter().filter_map(|session| session["name"].as_str().map(str::to_owned)).collect();
        for seen in fleet.remote_views() {
            for session in seen.snapshot.sessions.iter().filter(|session| named.insert(session.name.clone())) {
                sessions.push(remote_json(&seen, session));
            }
        }
    }
    Ok(Value::Array(sessions))
}

fn remote_json(seen: &Seen, session: &SessionView) -> Value {
    json!({
        "name": session.name,
        "provider": session.provider,
        "account": session.account,
        "status": session.status,
        "parent": session.parent,
        "cwd": session.cwd,
        "task_ref": session.tasks.first(),
        "tasks": session.tasks,
        "placement": session.placement,
        "machine": session.machine,
        "host": seen.snapshot.host,
        "remote": true,
        "seen_ms": seen.at_ms,
    })
}

pub(super) fn session_json(record: &SessionRecord, status: Status, waits: &Waits) -> Value {
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
        "tasks": record.tasks().collect::<Vec<_>>(),
        "created_ms": record.created_ms,
        "host_pid": record.host_pid,
        "placement": record.placement,
        "machine": record.machine.as_ref().map(|machine| &machine.name),
        "waiting": waits.of(&record.name).iter().map(|wait| wait.run.to_string()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::fleet::runtimes::Runtimes;

    #[test]
    fn the_whole_fleet_view_lists_the_sessions_every_relayed_host_runs_beside_this_hosts_own() {
        let home = tempfile::tempdir().unwrap();
        let ledger = Arc::new(crate::ledger::Ledger::open(&home.path().join("ledger.db")).unwrap());
        let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap();
        let remote = home.path().join(".local/state/north/fleet/.remote");
        std::fs::create_dir_all(&remote).unwrap();
        let view = json!({"ssh": "nexus", "at_ms": 7, "snapshot": {"host": "nexus", "gates": [], "sessions": [
            {"name": "north-316-d1325", "provider": "claude", "account": "a", "status": "idle", "parent": null, "cwd": "gex45:/home/tom/code/north/main", "placement": null, "machine": "gex45", "tasks": ["north#316", "north#317"]}
        ]}});
        std::fs::write(remote.join("nexus.json"), view.to_string()).unwrap();
        let plain = listing(&fleet, false).unwrap();
        assert!(plain.as_array().unwrap().is_empty(), "{plain}");
        let all = listing(&fleet, true).unwrap();
        let [session] = all.as_array().unwrap().as_slice() else { panic!("{all}") };
        assert_eq!((session["name"].as_str(), session["status"].as_str(), session["host"].as_str(), session["machine"].as_str()), (Some("north-316-d1325"), Some("idle"), Some("nexus"), Some("gex45")));
        assert_eq!((session["task_ref"].as_str(), session["tasks"].clone()), (Some("north#316"), json!(["north#316", "north#317"])));
    }
}
