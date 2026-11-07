//! The outcome of each capacity admission a host decided, kept so the ledger can weigh the admission subsystem by what it decided.
use rusqlite::{Connection, OptionalExtension, params};

use super::runs::row::Row;
use super::verb::Failure;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS capacity_admissions(
  id TEXT PRIMARY KEY,
  host TEXT NOT NULL,
  at TEXT NOT NULL,
  owner TEXT NOT NULL,
  class TEXT NOT NULL,
  decision TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  queued_seconds REAL NOT NULL DEFAULT 0,
  cpus REAL,
  memory_mib REAL
) STRICT";

pub const KIND: &str = "capacity";

pub fn ensure(db: &Connection) -> Result<(), Failure> {
    db.execute(SCHEMA, [])?;
    Ok(())
}

pub fn record(db: &Connection, row: &Row) -> Result<usize, Failure> {
    let field = |key: &str| row.get(key).and_then(|value| value.as_str()).map(str::to_owned).unwrap_or_default();
    let number = |key: &str| row.get(key).and_then(|value| value.as_f64());
    let (id, host, at, owner, class, decision) = (field("id"), field("host"), field("at"), field("owner"), field("class"), field("decision"));
    if [&id, &host, &at, &owner, &class, &decision].iter().any(|text| text.is_empty()) {
        return Ok(0);
    }
    Ok(db.execute(
        "INSERT OR IGNORE INTO capacity_admissions(id, host, at, owner, class, decision, reason, queued_seconds, cpus, memory_mib) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, host, at, owner, class, decision, field("reason"), number("queued_seconds").unwrap_or(0.0), number("cpus"), number("memory_mib")],
    )?)
}

/// Each host's newest recorded admission time, one `HOST<TAB>AT` line per host, so a host ships only what the ledger lacks.
pub fn seen(db: &Connection) -> Result<String, Failure> {
    if db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'capacity_admissions'", [], |_| Ok(())).optional()?.is_none() {
        return Ok(String::new());
    }
    let mut query = db.prepare("SELECT host, max(at) FROM capacity_admissions GROUP BY host ORDER BY host")?;
    let rows = query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut listed = String::new();
    for row in rows {
        let (host, at) = row?;
        listed += &format!("{host}\t{at}\n");
    }
    Ok(listed)
}
