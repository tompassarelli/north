use rusqlite::Connection;
use rusqlite::types::Value as Sql;
use serde_json::{Map, Value};

use crate::platform::usage_table;
use crate::usage::store::COLUMNS;

pub(super) type Reading = Map<String, Value>;

const IDENTITY: usize = 5;

pub(super) fn publish(db: &Connection, mirrored: bool, rows: &[Reading], ingest: &dyn Fn(&[u8]) -> Result<(), String>) -> Result<(), String> {
    if mirrored && !rows.is_empty() {
        let batch: String = rows
            .iter()
            .map(|row| {
                let mut tagged = row.clone();
                tagged.insert("type".to_owned(), Value::from("usage"));
                format!("{}\n", Value::Object(tagged))
            })
            .collect();
        ingest(batch.as_bytes())?;
    }
    record(db, rows)
}

pub(super) fn record(db: &Connection, rows: &[Reading]) -> Result<(), String> {
    let fail = |error: rusqlite::Error| error.to_string();
    usage_table::ensure(db).map_err(fail)?;
    let identity: Vec<String> = COLUMNS[..IDENTITY].iter().map(|name| format!("{name} IS ?")).collect();
    let insert = format!(
        "INSERT INTO usage({}) SELECT {} WHERE NOT EXISTS (SELECT 1 FROM usage WHERE {})",
        COLUMNS.join(", "),
        vec!["?"; COLUMNS.len()].join(", "),
        identity.join(" AND ")
    );
    let transaction = db.unchecked_transaction().map_err(fail)?;
    {
        let mut statement = transaction.prepare(&insert).map_err(fail)?;
        for row in rows {
            let values = COLUMNS.iter().chain(&COLUMNS[..IDENTITY]).map(|name| bind(row.get(*name))).collect::<Result<Vec<_>, _>>()?;
            statement.execute(rusqlite::params_from_iter(values)).map_err(fail)?;
        }
    }
    transaction.commit().map_err(fail)
}

fn bind(value: Option<&Value>) -> Result<Sql, String> {
    match value {
        None | Some(Value::Null) => Ok(Sql::Null),
        Some(Value::Bool(flag)) => Ok(Sql::Integer(i64::from(*flag))),
        Some(Value::String(text)) => Ok(Sql::Text(text.clone())),
        Some(Value::Number(number)) => number
            .as_i64()
            .map(Sql::Integer)
            .or_else(|| number.as_f64().filter(|_| !number.is_u64()).map(Sql::Real))
            .ok_or_else(|| format!("usage value {number} does not fit a column")),
        Some(other) => Err(format!("usage value {other} is not a scalar")),
    }
}
