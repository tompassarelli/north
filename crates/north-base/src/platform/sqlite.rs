use std::path::Path;
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Number, Value};

use crate::error::{NorthError, NorthResult};

#[derive(Clone, Copy)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

pub fn rows(db: &Path, access: Access, busy: Duration, query: &str) -> NorthResult<Vec<Value>> {
    let connection = open(db, access, busy)?;
    let mut statement = connection.prepare(query).map_err(|error| failure(db, error))?;
    let names: Vec<String> = statement.column_names().into_iter().map(str::to_owned).collect();
    let mut cursor = statement.query([]).map_err(|error| failure(db, error))?;
    let mut result = Vec::new();
    while let Some(row) = cursor.next().map_err(|error| failure(db, error))? {
        let mut object = Map::new();
        for (index, name) in names.iter().enumerate() {
            object.insert(name.clone(), json(row.get_ref(index).map_err(|error| failure(db, error))?));
        }
        result.push(Value::Object(object));
    }
    Ok(result)
}

pub fn execute(db: &Path, busy: Duration, script: &str) -> NorthResult<()> {
    open(db, Access::ReadWrite, busy)?.execute_batch(script).map_err(|error| failure(db, error))
}

fn open(db: &Path, access: Access, busy: Duration) -> NorthResult<Connection> {
    let flags = match access {
        Access::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
        Access::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(db, flags).map_err(|error| failure(db, error))?;
    connection.busy_timeout(busy).map_err(|error| failure(db, error))?;
    Ok(connection)
}

fn json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => Value::from(number),
        ValueRef::Real(number) => Number::from_f64(number).map_or(Value::Null, Value::Number),
        ValueRef::Text(text) => Value::String(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Blob(bytes) => Value::String(bytes.iter().map(|byte| char::from(*byte)).collect()),
    }
}

fn failure(db: &Path, error: rusqlite::Error) -> NorthError {
    NorthError::Configuration(format!("sqlite {}: {error}", db.display()))
}
