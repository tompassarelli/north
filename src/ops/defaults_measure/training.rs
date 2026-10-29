use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use crate::foundation::clock::Timestamp;

pub(super) fn iso_stamp(text: &str) -> Option<String> {
    let at = Timestamp::parse_python_datetime(text)?;
    Some(at.iso_micros_utc_offset().replace("+00:00", "Z"))
}

pub(super) fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
}

pub(super) fn training_db(path: &Path, cutoff: &str) -> Result<Connection, String> {
    let fail = |error: rusqlite::Error| format!("{}: {error}", path.display());
    let source = open_read_only(path).map_err(fail)?;
    let db = Connection::open_in_memory().map_err(fail)?;
    let schema: String = source.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'runs'", [], |row| row.get(0)).map_err(fail)?;
    db.execute(&schema, []).map_err(fail)?;
    let mut select = source.prepare("SELECT * FROM runs ORDER BY ended, agent").map_err(fail)?;
    let columns: Vec<String> = select.column_names().into_iter().map(str::to_owned).collect();
    let at = |name: &str| columns.iter().position(|column| column == name);
    let (ended, started, outcome, cost, landed, landed_at) = (at("ended"), at("started"), at("outcome"), at("cost_usd"), at("landed"), at("landed_at"));
    let insert = format!("INSERT INTO runs VALUES ({})", vec!["?"; columns.len()].join(","));
    let mut rows = select.query([]).map_err(fail)?;
    while let Some(row) = rows.next().map_err(fail)? {
        let mut values = (0..columns.len()).map(|index| row.get::<_, Value>(index)).collect::<rusqlite::Result<Vec<_>>>().map_err(fail)?;
        let stamp = |value: &Value| -> Result<Option<String>, String> {
            match value {
                Value::Text(text) if !text.is_empty() => iso_stamp(text).map(Some).ok_or_else(|| format!("{}: run time {text:?} is not ISO", path.display())),
                _ => Ok(None),
            }
        };
        let Some(end) = ended.map(|index| stamp(&values[index])).transpose()?.flatten() else { continue };
        let running = outcome.is_some_and(|index| matches!(&values[index], Value::Text(text) if text == "running"));
        if end.as_str() >= cutoff || running {
            continue;
        }
        if let Some(index) = ended {
            values[index] = Value::Text(end);
        }
        if let Some(index) = started
            && let Some(begin) = stamp(&values[index])?
        {
            values[index] = Value::Text(begin);
        }
        if let Some(index) = cost {
            values[index] = Value::Null;
        }
        if let Some(index) = landed_at {
            let landed_in_time = stamp(&values[index])?.is_some_and(|stamp| stamp.as_str() < cutoff);
            if !landed_in_time {
                values[index] = Value::Null;
                if let Some(flag) = landed {
                    values[flag] = Value::Integer(0);
                }
            }
        }
        db.execute(&insert, rusqlite::params_from_iter(values)).map_err(fail)?;
    }
    db.execute_batch("PRAGMA query_only = ON").map_err(fail)?;
    Ok(db)
}
