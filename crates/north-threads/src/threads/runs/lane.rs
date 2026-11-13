//! The lane a run works in and its tip, as the run's row names them (north#1339).
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, params};
use serde_json::Value;

use super::row::Row;
use crate::threads::verb::Failure;

pub(super) fn record(db: &Connection, r: &Row, agent: Sql) -> Result<(), Failure> {
    let Some(lane) = r.get("lane").and_then(Value::as_str) else { return Ok(()) };
    db.execute("UPDATE runs SET lane = ?, lane_tip = coalesce(?, lane_tip) WHERE agent = ?", params![lane, r.get("lane_tip").and_then(Value::as_str), agent])?;
    Ok(())
}
