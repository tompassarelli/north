use rusqlite::{Connection, OptionalExtension, params};

use super::row::Row;
use crate::threads::verb::Failure;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS hook_events(session TEXT NOT NULL, tool_use TEXT NOT NULL, tool TEXT NOT NULL, event TEXT NOT NULL, verdict TEXT NOT NULL CHECK(verdict IN ('refusal','rewrite','advisory')), rule TEXT NOT NULL, at TEXT NOT NULL, turn INTEGER NOT NULL, PRIMARY KEY(session, tool_use, verdict, rule)) STRICT";

#[derive(Debug, Clone, PartialEq)]
pub struct HookEvent {
    pub session: String,
    pub tool: String,
    pub verdict: String,
    pub rule: String,
    pub at: String,
    pub turn: i64,
}

pub fn ensure(db: &Connection) -> Result<(), Failure> {
    db.execute(SCHEMA, [])?;
    Ok(())
}

pub fn record(db: &Connection, row: &Row) -> Result<usize, Failure> {
    let field = |key: &str| row.get(key).and_then(|value| value.as_str()).map(str::to_owned).unwrap_or_default();
    let turn = row.get("turn").and_then(|value| value.as_f64()).map_or(0, |turn| turn as i64);
    let (session, tool_use, at) = (field("session"), field("tool_use"), field("at"));
    if session.is_empty() || tool_use.is_empty() || at.is_empty() {
        return Ok(0);
    }
    Ok(db.execute(
        "INSERT OR IGNORE INTO hook_events(session, tool_use, tool, event, verdict, rule, at, turn) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![session, tool_use, field("tool"), field("event"), field("verdict"), field("rule"), at, turn],
    )?)
}

pub fn load(db: &Connection, since: &str) -> Result<Vec<HookEvent>, String> {
    let failed = |error: rusqlite::Error| format!("ledger: {error}");
    if db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'hook_events'", [], |_| Ok(())).optional().map_err(failed)?.is_none() {
        return Ok(Vec::new());
    }
    let mut query = db.prepare("SELECT session, tool, verdict, rule, at, turn FROM hook_events WHERE at >= ?1").map_err(failed)?;
    let rows = query
        .query_map([since], |row| Ok(HookEvent { session: row.get(0)?, tool: row.get(1)?, verdict: row.get(2)?, rule: row.get(3)?, at: row.get(4)?, turn: row.get(5)? }))
        .map_err(failed)?;
    rows.collect::<Result<_, _>>().map_err(failed)
}
