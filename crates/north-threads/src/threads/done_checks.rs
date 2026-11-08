use rusqlite::{Connection, params};

use super::runs::row::Row;
use crate::platform::pyjson::Value;
use super::verb::Failure;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS done_checks(
  host TEXT NOT NULL,
  task_ref TEXT NOT NULL,
  head_sha TEXT NOT NULL,
  command TEXT NOT NULL,
  exit INTEGER NOT NULL,
  ran_ms INTEGER NOT NULL,
  PRIMARY KEY(host, ran_ms, head_sha, command)
) STRICT";

pub const KIND: &str = "done_check";

pub fn ensure(db: &Connection) -> Result<(), Failure> {
    db.execute(SCHEMA, [])?;
    Ok(())
}

pub fn record(db: &Connection, row: &Row) -> Result<usize, Failure> {
    let field = |key: &str| row.get(key).and_then(|value| value.as_str()).map(str::to_owned).unwrap_or_default();
    let number = |key: &str| match row.get(key) {
        Some(Value::Number { raw, .. }) => raw.parse::<i64>().ok(),
        _ => None,
    };
    let (host, task_ref, head_sha, command) = (field("host"), field("task_ref"), field("head_sha").to_ascii_lowercase(), field("command"));
    let (Some(exit), Some(ran_ms)) = (number("exit"), number("ran_ms")) else { return Ok(0) };
    if [&host, &task_ref, &head_sha, &command].iter().any(|text| text.is_empty()) {
        return Ok(0);
    }
    Ok(db.execute(
        "INSERT OR IGNORE INTO done_checks(host, task_ref, head_sha, command, exit, ran_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![host, task_ref, head_sha, command, exit, ran_ms],
    )?)
}
