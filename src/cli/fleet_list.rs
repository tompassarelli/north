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
fn elsewhere(fleet: &Fleet, named: &BTreeSet<String>) -> Vec<SessionRow> {
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
    let mut sessions: Vec<Value> = fleet.sessions()?.iter().map(|record| session_json(record, fleet.status(record), &waits)).collect();
    if all {
        for record in fleet.past_sessions() {
            let mut session = session_json(&record, Status::Exited, &waits);
            session["past"] = json!(true);
            sessions.push(session);
        }
    }
    if all {
        let named: BTreeSet<String> = sessions.iter().filter_map(|session| session["name"].as_str().map(str::to_owned)).collect();
        for row in elsewhere(fleet, &named) {
            sessions.push(remote_json(&row));
        }
    }
    Ok(Value::Array(sessions))
}

fn remote_json(row: &SessionRow) -> Value {
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
        "host": row.host,
        "remote": true,
        "seen_ms": updated_ms(row),
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
        "ended": record.ended.map(Ending::label),
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

    fn table_row(name: &str, host: &str, status: &str, updated_ms: u64) -> SessionRow {
        let updated = Timestamp::from_unix_millis(i64::try_from(updated_ms).unwrap()).iso();
        SessionRow {
            name: name.into(),
            host: host.into(),
            status: status.into(),
            created: updated.clone(),
            updated,
            provider: "claude".into(),
            account: "a".into(),
            cwd: "/home/tom/code/north/main".into(),
            machine: Some("gex45".into()),
            tasks: vec!["north#316".into(), "north#317".into()],
            ..SessionRow::default()
        }
    }

    fn fleet_at(home: &std::path::Path) -> Fleet {
        let ledger = Arc::new(crate::ledger::Ledger::open(&home.join("ledger.db")).unwrap());
        Fleet::at(home.into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap().hosted_as("laptop")
    }

    #[test]
    fn the_whole_fleet_view_lists_the_sessions_the_fleet_table_holds_for_other_hosts_beside_this_hosts_own() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet_at(home.path());
        let now = now_ms();
        let rows = [table_row("north-316-d1325", "nexus", "idle", now), table_row("mine", "laptop", "working", now)];
        crate::fleet::store::cache_sessions(&home.path().join(".local/state/north/fleet"), &rows).unwrap();
        let plain = listing(&fleet, false).unwrap();
        assert!(plain.as_array().unwrap().is_empty(), "{plain}");
        let all = listing(&fleet, true).unwrap();
        let [session] = all.as_array().unwrap().as_slice() else { panic!("{all}") };
        assert_eq!((session["name"].as_str(), session["status"].as_str(), session["host"].as_str(), session["machine"].as_str()), (Some("north-316-d1325"), Some("idle"), Some("nexus"), Some("gex45")));
        assert_eq!((session["task_ref"].as_str(), session["tasks"].clone()), (Some("north#316"), json!(["north#316", "north#317"])));
    }

    #[test]
    fn this_hosts_live_record_wins_over_its_own_table_row_and_another_hosts_row_comes_from_the_table() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet_at(home.path());
        let now = now_ms();
        let rows = [table_row("mine", "laptop", "working", now), table_row("held", "nexus", "idle", now), table_row("shadow", "nexus", "idle", now - 8 * 24 * 60 * 60 * 1000)];
        let named = BTreeSet::from(["held".to_string()]);
        crate::fleet::store::cache_sessions(&home.path().join(".local/state/north/fleet"), &rows).unwrap();
        assert!(elsewhere(&fleet, &named).is_empty(), "a name this host runs, this host's own row and a long-silent row are not listed from the table");
        let [other] = elsewhere(&fleet, &BTreeSet::new()).try_into().unwrap_or_else(|rows: Vec<SessionRow>| panic!("{rows:?}"));
        assert_eq!(other.name, "held");
        assert_eq!(remote_line(&other, now), "held\tclaude\ta\tidle\tnexus:gex45:/home/tom/code/north/main\t-");
        assert_eq!(remote_line(&other, now + 12 * 60_000), "held\tclaude\ta\tidle (last seen 12m ago)\tnexus:gex45:/home/tom/code/north/main\t-");
    }
}
