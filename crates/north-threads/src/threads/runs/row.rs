//! One ingested JSON row, its fields read as Python's `dict` reads them and bound as sqlite3 binds them.
use std::collections::BTreeMap;

use rusqlite::types::Value as Sql;

use serde_json::Value;
use crate::threads::verb::{Failure, integer};

pub struct Row(BTreeMap<String, Value>);

impl Row {
    pub fn parse(line: &[u8]) -> Result<Self, Failure> {
        let value: Value = serde_json::from_slice(line).map_err(|_| Failure::Refused(format!("unreadable ingest row: {}", String::from_utf8_lossy(line).trim())))?;
        let Value::Object(fields) = value else { return Err(Failure::Refused("an ingest row is not a JSON object".into())) };
        Ok(Self(fields.into_iter().collect()))
    }

    /// `r.get(key)`: None when absent or null.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key).filter(|value| !value.is_null())
    }

    /// `r.get(key, default)`: the default only when the key is absent.
    pub fn get_or(&self, key: &str, default: Value) -> Value {
        self.0.get(key).cloned().unwrap_or(default)
    }

    pub fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// `r[key]`, refused when absent.
    pub fn require(&self, key: &str) -> Result<&Value, Failure> {
        self.0.get(key).ok_or_else(|| Failure::Refused(format!("an ingest row has no {key}")))
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    pub fn truthy(&self, key: &str) -> bool {
        self.get(key).is_some_and(truthy)
    }

    /// `r.get(key) or default`.
    pub fn or(&self, key: &str, default: Value) -> Value {
        self.get(key).filter(|value| truthy(value)).cloned().unwrap_or(default)
    }

    /// `r.get(key)` as sqlite3 binds it.
    pub fn sql(&self, key: &str) -> Result<Sql, Failure> {
        bind(self.get(key))
    }

    /// `r[key]` as sqlite3 binds it.
    pub fn required(&self, key: &str) -> Result<Sql, Failure> {
        bind(Some(self.require(key)?))
    }

    /// `r.get("fast", 0)` as sqlite3 binds it.
    pub fn fast(&self) -> Result<Sql, Failure> {
        bind(Some(&self.get_or("fast", integer(0))))
    }

    /// The skills the run loaded as the JSON list the ledger keeps, or NULL when the row names none.
    pub fn skills(&self) -> Sql {
        match self.get("skills") {
            Some(skills @ Value::Array(_)) => Sql::Text(skills.to_string()),
            _ => Sql::Null,
        }
    }

    /// `r.get(key) and round(r[key])`.
    pub fn rounded(&self, key: &str) -> Result<Sql, Failure> {
        match self.get(key) {
            Some(value) if truthy(value) => round(value),
            other => bind(other),
        }
    }

    /// `None if r.get(key) is None else round(r[key])`.
    pub fn round(&self, key: &str) -> Result<Sql, Failure> {
        self.get(key).map_or(Ok(Sql::Null), round)
    }
}

pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

pub fn bind(value: Option<&Value>) -> Result<Sql, Failure> {
    Ok(match value {
        None | Some(Value::Null) => Sql::Null,
        Some(Value::Bool(value)) => Sql::Integer(i64::from(*value)),
        Some(Value::Number(number)) if number.is_f64() => Sql::Real(number.as_f64().unwrap_or_default()),
        Some(Value::Number(number)) => Sql::Integer(number.as_i64().ok_or_else(|| unbindable(&number.to_string()))?),
        Some(Value::String(text)) => Sql::Text(text.clone()),
        Some(other) => return Err(unbindable(&other.to_string())),
    })
}

/// `bind` over text that may be absent.
pub fn text(value: Option<&str>) -> Sql {
    value.map_or(Sql::Null, |text| Sql::Text(text.to_owned()))
}

/// Python's `round`, which rounds half to even.
fn round(value: &Value) -> Result<Sql, Failure> {
    match value {
        Value::Bool(value) => Ok(Sql::Integer(i64::from(*value))),
        Value::Number(number) if number.is_f64() => Ok(Sql::Integer(number.as_f64().unwrap_or_default().round_ties_even() as i64)),
        Value::Number(_) => bind(Some(value)),
        other => Err(Failure::Refused(format!("an ingest row's ETA is not a number: {other}"))),
    }
}

fn unbindable(text: &str) -> Failure {
    Failure::Refused(format!("an ingest row holds a value sqlite cannot store: {text}"))
}

/// A ledger value as `json.loads` would have read it.
pub fn value(sql: &Sql) -> Value {
    match sql {
        Sql::Null | Sql::Blob(_) => Value::Null,
        Sql::Integer(number) => integer(*number),
        Sql::Real(number) => Value::from(*number),
        Sql::Text(text) => Value::from(text.as_str()),
    }
}

/// Python's `repr` of a JSON value, for the refusal that names it.
pub fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
            let escaped: String = text.chars().map(|c| escape(c, quote)).collect();
            format!("{quote}{escaped}{quote}")
        }
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}

fn escape(c: char, quote: char) -> String {
    match c {
        '\\' => "\\\\".to_owned(),
        '\n' => "\\n".to_owned(),
        '\r' => "\\r".to_owned(),
        '\t' => "\\t".to_owned(),
        c if c == quote => format!("\\{c}"),
        c if (c as u32) < 0x20 || c as u32 == 0x7f => format!("\\x{:02x}", c as u32),
        c => c.to_string(),
    }
}
