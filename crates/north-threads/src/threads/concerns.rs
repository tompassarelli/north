use rusqlite::{Connection, params};

use super::runs::row::Row;
use super::verb::Failure;
use crate::foundation::clock::Timestamp;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS concern_samples(
  project TEXT NOT NULL,
  concern TEXT NOT NULL,
  sampler TEXT NOT NULL,
  host TEXT NOT NULL,
  at TEXT NOT NULL,
  value REAL,
  why TEXT NOT NULL DEFAULT '',
  detail TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL CHECK(state IN ('green','drifting','red','repairing','unmeasured')),
  PRIMARY KEY(project, concern, host, at)
) STRICT";
const KEPT_SECONDS: i64 = 35 * 86_400;
const STATES: [&str; 5] = ["green", "drifting", "red", "repairing", "unmeasured"];

pub const KIND: &str = "concern";

pub fn ensure(db: &Connection, now: Timestamp) -> Result<(), Failure> {
    db.execute(SCHEMA, [])?;
    db.execute("DELETE FROM concern_samples WHERE at < ?1", [Timestamp::from_unix_seconds(now.unix_seconds() - KEPT_SECONDS).iso()])?;
    Ok(())
}

pub fn record(db: &Connection, row: &Row) -> Result<usize, Failure> {
    let field = |key: &str| row.get(key).and_then(|value| value.as_str()).map(str::to_owned).unwrap_or_default();
    let (project, concern, sampler, host, state) = (field("project"), field("concern"), field("sampler"), field("host"), field("state"));
    let Some(at) = Timestamp::parse_rfc3339(&field("at")) else { return Ok(0) };
    if [&project, &concern, &sampler, &host].iter().any(|text| text.is_empty()) || !STATES.contains(&state.as_str()) {
        return Ok(0);
    }
    let value = row.get("value").and_then(|value| value.as_f64());
    Ok(db.execute(
        "INSERT OR IGNORE INTO concern_samples(project, concern, sampler, host, at, value, why, detail, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![project, concern, sampler, host, at.iso(), value, field("why"), field("detail"), state],
    )?)
}
