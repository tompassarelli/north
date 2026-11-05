use std::process::Command;

use rusqlite::{Connection, params_from_iter};
use serde_json::{Map, Value};

use super::database::{capacity_database, sql};
use crate::platform::disk_budget::policy_path;

pub(super) fn run_columns(db: &Connection) -> Vec<String> {
    db.prepare("PRAGMA table_info(capacity_runs)")
        .and_then(|mut statement| statement.query_map([], |row| row.get::<_, String>(1)).map(|names| names.flatten().collect()))
        .unwrap_or_default()
}

pub(super) fn claim_local_rows(db: &Connection, local: &str) -> rusqlite::Result<()> {
    db.execute("UPDATE capacity_runs SET host = ? WHERE host IS NULL", [local])?;
    Ok(())
}

pub(super) fn insert_row(db: &Connection, columns: &[String], row: &Map<String, Value>) -> rusqlite::Result<bool> {
    let keys: Vec<&String> = columns.iter().filter(|column| row.contains_key(column.as_str())).collect();
    let names = keys.iter().map(|key| key.as_str()).collect::<Vec<_>>().join(",");
    let placeholders = vec!["?"; keys.len()].join(",");
    let mut statement = db.prepare_cached(&format!("INSERT OR IGNORE INTO capacity_runs ({names}) VALUES ({placeholders})"))?;
    Ok(statement.execute(params_from_iter(keys.iter().map(|key| sql(row.get(key.as_str())))))? > 0)
}

pub(super) fn last_at(db: &Connection, filter: &str, argument: &str) -> rusqlite::Result<Option<String>> {
    db.query_row(&format!("SELECT max(at) FROM capacity_runs WHERE {filter}"), [argument], |row| row.get(0))
}

fn ssh_hosts(policy: &str, local: &str) -> Vec<(String, String)> {
    let Ok(document) = policy.parse::<toml_edit::DocumentMut>() else { return Vec::new() };
    let Some(hosts) = document.get("hosts").and_then(|hosts| hosts.as_table()) else { return Vec::new() };
    hosts
        .iter()
        .filter(|(name, _)| *name != local)
        .filter_map(|(name, host)| {
            let destination = ["ssh", "deploy_ssh"].into_iter().find_map(|key| host.get(key)?.as_str())?;
            Some((name.to_owned(), destination.to_owned()))
        })
        .collect()
}

const REMOTE_DATABASE: &str = "\"${XDG_STATE_HOME:-$HOME/.local/state}/agents/machine-capacity/capacity.db\"";

fn ssh_fetch(destination: &str, since: &str) -> Result<String, String> {
    let query = format!("SELECT * FROM capacity_runs WHERE at >= '{since}' ORDER BY at");
    let remote = format!("sqlite3 -readonly -cmd '.timeout 5000' -json {REMOTE_DATABASE} \"{query}\"");
    let output = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", destination, &remote])
        .output()
        .map_err(|error| format!("ssh: {error}"))?;
    if !output.status.success() {
        return Err(format!("ssh {destination} exited with {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn pull_host(db: &mut Connection, name: &str, fetch: &dyn Fn(&str) -> Result<String, String>) -> Result<usize, String> {
    let since = last_at(db, "host = ?", name).map_err(|error| error.to_string())?.unwrap_or_default();
    if !since.bytes().all(|byte| byte.is_ascii_digit() || b"-:.TZ".contains(&byte)) {
        return Err(format!("{name}: last pulled time {since:?} is not an ISO time"));
    }
    let text = fetch(&since)?;
    if text.trim().is_empty() {
        return Ok(0);
    }
    let rows: Vec<Map<String, Value>> = serde_json::from_str(&text).map_err(|error| format!("{name}: unreadable rows: {error}"))?;
    let columns = run_columns(db);
    let transaction = db.transaction().map_err(|error| error.to_string())?;
    let mut added = 0;
    for mut row in rows {
        let Some(source) = row.get("source_id").and_then(Value::as_str).map(|source| format!("{name}:{source}")) else { continue };
        row.insert("source_id".to_owned(), Value::String(source));
        row.insert("host".to_owned(), Value::String(name.to_owned()));
        added += usize::from(insert_row(&transaction, &columns, &row).map_err(|error| error.to_string())?);
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(added)
}

pub fn open_fleet_database(local: &str) -> rusqlite::Result<Connection> {
    let db = capacity_database()?;
    claim_local_rows(&db, local)?;
    Ok(db)
}

pub fn run(arguments: &[String]) -> u8 {
    let only = match arguments {
        [] => None,
        [flag, name] if flag == "--host" => Some(name.as_str()),
        _ => {
            eprintln!("usage: north capacity pull [--host NAME]");
            return 2;
        }
    };
    let local = crate::platform::host::name();
    let policy = std::fs::read_to_string(policy_path()).unwrap_or_default();
    let mut db = match open_fleet_database(&local) {
        Ok(db) => db,
        Err(error) => {
            eprintln!("north capacity pull: {error}");
            return 1;
        }
    };
    let mut failed = false;
    for (name, destination) in ssh_hosts(&policy, &local).into_iter().filter(|(name, _)| only.is_none_or(|only| only == name)) {
        match pull_host(&mut db, &name, &|since| ssh_fetch(&destination, since)) {
            Ok(added) => println!("{name}: {added} new rows"),
            Err(message) => {
                eprintln!("north capacity pull: {name}: {message}");
                failed = true;
            }
        }
    }
    u8::from(failed)
}

#[cfg(test)]
#[path = "fleet_pull_tests.rs"]
mod tests;
