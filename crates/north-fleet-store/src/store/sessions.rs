use rusqlite::{Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

use crate::status::Status;
use crate::runtime::PROXY;
use super::schema::sql;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionRow {
    pub name: String,
    pub host: String,
    pub status: String,
    pub created: String,
    pub updated: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub placement: Option<String>,
    #[serde(default)]
    pub machine: Option<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
    #[serde(default)]
    pub conversation: Option<String>,
}

const COLUMNS: &str = "name, host, status, created, updated, provider, account, cwd, parent, placement, machine, tasks, conversation";

const UPSERT: &str = "INSERT INTO fleet_sessions(name, host, status, created, updated, provider, account, cwd, parent, placement, machine, tasks, conversation) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) \
     ON CONFLICT(name) DO UPDATE SET host = excluded.host, status = excluded.status, created = excluded.created, updated = excluded.updated, \
     provider = excluded.provider, account = excluded.account, cwd = excluded.cwd, parent = excluded.parent, placement = excluded.placement, machine = excluded.machine, tasks = excluded.tasks, conversation = excluded.conversation";

fn upsert(tx: &Transaction, row: &SessionRow, guard: &str) -> NorthResult<usize> {
    tx.execute(
        &format!("{UPSERT} WHERE {guard}"),
        params![row.name, row.host, row.status, row.created, row.updated, Status::Exited.label(), row.provider, row.account, row.cwd, row.parent, row.placement, row.machine, serde_json::to_string(&row.tasks)?, row.conversation],
    )
    .map_err(sql)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Claim {
    Fresh,
    TakeOver,
}

pub(super) fn register(tx: &Transaction, row: &SessionRow, claim: Claim, slots: &Slots) -> NorthResult<()> {
    refuse_past(tx, row, slots)?;
    let guard = match claim {
        Claim::Fresh => "fleet_sessions.status = ?6 OR fleet_sessions.host = excluded.host",
        Claim::TakeOver => "1",
    };
    if upsert(tx, row, guard)? == 1 {
        return Ok(());
    }
    let (host, status): (String, String) =
        tx.query_row("SELECT host, status FROM fleet_sessions WHERE name = ?1", [&row.name], |held| Ok((held.get(0)?, held.get(1)?))).map_err(sql)?;
    Err(NorthError::Refused(format!("the session name {} is held fleet-wide by a {status} session on {host}", row.name)))
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Slots {
    pub workers: Option<usize>,
    pub resident: Option<(String, usize)>,
}

const ACTIVE: &str = "status IN ('starting', 'working') AND name != ?1";

fn refuse_past(tx: &Transaction, row: &SessionRow, slots: &Slots) -> NorthResult<()> {
    let count = |filter: &str, value: &str| -> NorthResult<usize> {
        tx.query_row(&format!("SELECT count(*) FROM fleet_sessions WHERE {ACTIVE} AND {filter}"), params![row.name, value], |found| found.get::<_, i64>(0)).map(|found| usize::try_from(found).unwrap_or(usize::MAX)).map_err(sql)
    };
    if let Some(cap) = slots.workers {
        let working = count("name != ?2 AND COALESCE(NULLIF(provider, ''), 'claude') = 'claude' AND COALESCE(placement, '') NOT LIKE 'adopted window%'", PROXY)?;
        if working >= cap {
            return Err(NorthError::Refused(format!("spend cap: {working} worker sessions are starting or working across the fleet, the cap is {cap}")));
        }
    }
    if let Some((machine, cap)) = &slots.resident {
        let resident = count("COALESCE(machine, host) = ?2", machine)?;
        if resident >= *cap {
            return Err(NorthError::Refused(format!("resident cap: {resident} sessions are starting or working on {machine}, its resident cap is {cap}")));
        }
    }
    Ok(())
}

pub(super) fn publish(tx: &Transaction, rows: &[SessionRow], outranks: &[String]) -> NorthResult<Vec<String>> {
    let mut held_elsewhere = Vec::new();
    let outranked: Vec<String> = outranks.iter().map(|host| format!("'{}'", host.replace('\'', "''"))).collect();
    let guard = format!("fleet_sessions.host = excluded.host OR fleet_sessions.status = ?6 OR fleet_sessions.host IN ({})", if outranked.is_empty() { "NULL".to_owned() } else { outranked.join(", ") });
    for row in rows {
        if upsert(tx, row, &guard)? == 0 {
            held_elsewhere.push(row.name.clone());
        }
    }
    Ok(held_elsewhere)
}

pub(super) fn retire_vanished(tx: &Transaction, host: &str, rows: &[SessionRow], launching_since: &str) -> NorthResult<()> {
    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    tx.execute(
        "DELETE FROM fleet_sessions WHERE host = ?1 AND name NOT IN (SELECT value FROM json_each(?2)) AND NOT (status = ?3 AND updated >= ?4)",
        params![host, serde_json::to_string(&names)?, Status::Starting.label(), launching_since],
    )
    .map_err(sql)?;
    Ok(())
}

pub(super) fn retire_unplaced(tx: &Transaction, placed: &[String], publishing: Option<&str>) -> NorthResult<Vec<String>> {
    let mut statement = tx.prepare("DELETE FROM fleet_sessions WHERE host NOT IN (SELECT value FROM json_each(?1)) AND host IS NOT ?2 RETURNING name").map_err(sql)?;
    let retired = statement.query_map(params![serde_json::to_string(placed)?, publishing], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    Ok(retired)
}

pub(super) fn all(db:&rusqlite::Connection) -> NorthResult<Vec<SessionRow>> {
    let mut statement = db.prepare(&format!("SELECT {COLUMNS} FROM fleet_sessions ORDER BY name")).map_err(sql)?;
    let rows = statement.query_map([], held).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    Ok(rows)
}

fn held(row: &Row) -> rusqlite::Result<SessionRow> {
    let tasks: String = row.get(11)?;
    Ok(SessionRow {
        name: row.get(0)?,
        host: row.get(1)?,
        status: row.get(2)?,
        created: row.get(3)?,
        updated: row.get(4)?,
        provider: row.get(5)?,
        account: row.get(6)?,
        cwd: row.get(7)?,
        parent: row.get(8)?,
        placement: row.get(9)?,
        machine: row.get(10)?,
        tasks: serde_json::from_str(&tasks).map_err(|error| rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(error)))?,
        conversation: row.get(12)?,
    })
}
