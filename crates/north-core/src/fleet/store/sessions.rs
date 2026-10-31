use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

use super::super::model::Status;
use super::schema::sql;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionRow {
    pub name: String,
    pub host: String,
    pub status: String,
    pub created: String,
    pub updated: String,
}

const UPSERT: &str = "INSERT INTO fleet_sessions(name, host, status, created, updated) VALUES (?1, ?2, ?3, ?4, ?5) \
     ON CONFLICT(name) DO UPDATE SET host = excluded.host, status = excluded.status, created = excluded.created, updated = excluded.updated";

pub(super) fn register(tx: &Transaction, row: &SessionRow) -> NorthResult<()> {
    let launched = tx
        .execute(&format!("{UPSERT} WHERE fleet_sessions.status = ?6"), params![row.name, row.host, row.status, row.created, row.updated, Status::Exited.label()])
        .map_err(sql)?;
    if launched == 1 {
        return Ok(());
    }
    let (host, status): (String, String) =
        tx.query_row("SELECT host, status FROM fleet_sessions WHERE name = ?1", [&row.name], |held| Ok((held.get(0)?, held.get(1)?))).map_err(sql)?;
    Err(NorthError::Refused(format!("the session name {} is held fleet-wide by a {status} session on {host}", row.name)))
}

pub(super) fn publish(tx: &Transaction, rows: &[SessionRow]) -> NorthResult<Vec<String>> {
    let mut held_elsewhere = Vec::new();
    for row in rows {
        let published = tx
            .execute(
                &format!("{UPSERT} WHERE fleet_sessions.host = excluded.host OR fleet_sessions.status = ?6"),
                params![row.name, row.host, row.status, row.created, row.updated, Status::Exited.label()],
            )
            .map_err(sql)?;
        if published == 0 {
            held_elsewhere.push(row.name.clone());
        }
    }
    Ok(held_elsewhere)
}

pub(super) fn all(db: &rusqlite::Connection) -> NorthResult<Vec<SessionRow>> {
    let mut statement = db.prepare("SELECT name, host, status, created, updated FROM fleet_sessions ORDER BY name").map_err(sql)?;
    let rows = statement
        .query_map([], |row| Ok(SessionRow { name: row.get(0)?, host: row.get(1)?, status: row.get(2)?, created: row.get(3)?, updated: row.get(4)? }))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    Ok(rows)
}
