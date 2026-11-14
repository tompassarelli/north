//! The issue the 24-hour closer filed in place of each issue it closed with open boxes (north#1495). Only `north issue close-stale`, on the ledger host, writes this table and no threads verb does, so a lead's launch grant trusts it where it trusts no text on GitHub.
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rewrites(closed TEXT PRIMARY KEY CHECK(closed GLOB '*#[0-9]*'), successor TEXT NOT NULL CHECK(successor GLOB '*#[0-9]*'), at TEXT NOT NULL) STRICT";

/// Records that the closer filed `successor` in place of `closed`, both `REPO#N`; a second record of `closed` keeps the first.
pub fn record(db: &Connection, closed: &str, successor: &str, at: &str) -> rusqlite::Result<()> {
    db.execute_batch(SCHEMA)?;
    db.execute("INSERT OR IGNORE INTO rewrites(closed, successor, at) VALUES (?1, ?2, ?3)", params![closed, successor, at]).map(drop)
}

/// The issue the closer filed in place of `closed`, if it filed one.
pub fn successor(db: &Connection, closed: &str) -> rusqlite::Result<Option<String>> {
    if !kept(db)? {
        return Ok(None);
    }
    db.query_row("SELECT successor FROM rewrites WHERE closed = ?1", [closed], |row| row.get(0)).optional()
}

/// Every rewrite the closer recorded, as (closed, successor).
pub fn all(db: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    if !kept(db)? {
        return Ok(Vec::new());
    }
    let mut statement = db.prepare("SELECT closed, successor FROM rewrites")?;
    statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
}

fn kept(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'rewrites'", [], |row| row.get::<_, i64>(0)).map(|count| count > 0)
}
