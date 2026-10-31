use rusqlite::{Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

use super::super::model::Status;
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
}

const COLUMNS: &str = "name, host, status, created, updated, provider, account, cwd, parent, placement, machine, tasks";

const UPSERT: &str = "INSERT INTO fleet_sessions(name, host, status, created, updated, provider, account, cwd, parent, placement, machine, tasks) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
     ON CONFLICT(name) DO UPDATE SET host = excluded.host, status = excluded.status, created = excluded.created, updated = excluded.updated, \
     provider = excluded.provider, account = excluded.account, cwd = excluded.cwd, parent = excluded.parent, placement = excluded.placement, machine = excluded.machine, tasks = excluded.tasks";

fn upsert(tx: &Transaction, row: &SessionRow, guard: &str) -> NorthResult<usize> {
    tx.execute(
        &format!("{UPSERT} WHERE {guard}"),
        params![row.name, row.host, row.status, row.created, row.updated, Status::Exited.label(), row.provider, row.account, row.cwd, row.parent, row.placement, row.machine, serde_json::to_string(&row.tasks)?],
    )
    .map_err(sql)
}

pub(super) fn register(tx: &Transaction, row: &SessionRow) -> NorthResult<()> {
    if upsert(tx, row, "fleet_sessions.status = ?6")? == 1 {
        return Ok(());
    }
    let (host, status): (String, String) =
        tx.query_row("SELECT host, status FROM fleet_sessions WHERE name = ?1", [&row.name], |held| Ok((held.get(0)?, held.get(1)?))).map_err(sql)?;
    Err(NorthError::Refused(format!("the session name {} is held fleet-wide by a {status} session on {host}", row.name)))
}

pub(super) fn publish(tx: &Transaction, rows: &[SessionRow]) -> NorthResult<Vec<String>> {
    let mut held_elsewhere = Vec::new();
    for row in rows {
        if upsert(tx, row, "fleet_sessions.host = excluded.host OR fleet_sessions.status = ?6")? == 0 {
            held_elsewhere.push(row.name.clone());
        }
    }
    Ok(held_elsewhere)
}

pub(super) fn all(db: &rusqlite::Connection) -> NorthResult<Vec<SessionRow>> {
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
    })
}
