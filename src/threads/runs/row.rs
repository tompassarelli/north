//! One ingested JSON row, its fields read as Python's `dict` reads them and bound as sqlite3 binds them.
use std::collections::BTreeMap;

use rusqlite::types::Value as Sql;

use crate::platform::pyjson::{self, Value};
use crate::threads::verb::{Failure, integer};

pub(crate) struct Row(BTreeMap<String, Value>);

impl Row {
    pub(crate) fn parse(line: &[u8]) -> Result<Self, Failure> {
        let value = pyjson::loads(line).map_err(|_| Failure::Refused(format!("unreadable ingest row: {}", String::from_utf8_lossy(line).trim())))?;
        let Some(fields) = value.as_object() else { return Err(Failure::Refused("an ingest row is not a JSON object".into())) };
        Ok(Self(fields.iter().map(|(key, value)| (key.lossy().into_owned(), value.clone())).collect()))
    }

    /// `r.get(key)`: None when absent or null.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key).filter(|value| !value.is_null())
    }

    /// `r.get(key, default)`: the default only when the key is absent.
    pub(crate) fn get_or(&self, key: &str, default: Value) -> Value {
        self.0.get(key).cloned().unwrap_or(default)
    }

    pub(crate) fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// `r[key]`, refused when absent.
    pub(crate) fn require(&self, key: &str) -> Result<&Value, Failure> {
        self.0.get(key).ok_or_else(|| Failure::Refused(format!("an ingest row has no {key}")))
    }

    pub(crate) fn set(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    pub(crate) fn truthy(&self, key: &str) -> bool {
        self.get(key).is_some_and(truthy)
    }

    /// `r.get(key) or default`.
    pub(crate) fn or(&self, key: &str, default: Value) -> Value {
        self.get(key).filter(|value| truthy(value)).cloned().unwrap_or(default)
    }

    /// `r.get(key)` as sqlite3 binds it.
    pub(crate) fn sql(&self, key: &str) -> Result<Sql, Failure> {
        bind(self.get(key))
    }

    /// `r[key]` as sqlite3 binds it.
    pub(crate) fn required(&self, key: &str) -> Result<Sql, Failure> {
        bind(Some(self.require(key)?))
    }

    /// `r.get("fast", 0)` as sqlite3 binds it.
    pub(crate) fn fast(&self) -> Result<Sql, Failure> {
        bind(Some(&self.get_or("fast", integer(0))))
    }

    /// `r.get(key) and round(r[key])`.
    pub(crate) fn rounded(&self, key: &str) -> Result<Sql, Failure> {
        match self.get(key) {
            Some(value) if truthy(value) => round(value),
            other => bind(other),
        }
    }

    /// `None if r.get(key) is None else round(r[key])`.
    pub(crate) fn round(&self, key: &str) -> Result<Sql, Failure> {
        self.get(key).map_or(Ok(Sql::Null), round)
    }
}

pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number { raw, .. } => raw.parse::<f64>() != Ok(0.0),
        Value::String(text) => !text.codepoints.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

fn is_float(raw: &str) -> bool {
    raw.contains(['.', 'e', 'E', 'N', 'I'])
}

pub(crate) fn bind(value: Option<&Value>) -> Result<Sql, Failure> {
    Ok(match value {
        None | Some(Value::Null) => Sql::Null,
        Some(Value::Bool(value)) => Sql::Integer(i64::from(*value)),
        Some(Value::Number { raw, .. }) if is_float(raw) => Sql::Real(raw.parse().map_err(|_| unbindable(raw))?),
        Some(Value::Number { raw, .. }) => Sql::Integer(raw.parse().map_err(|_| unbindable(raw))?),
        Some(Value::String(text)) => Sql::Text(text.lossy().into_owned()),
        Some(other) => return Err(unbindable(&pyjson::dumps(other))),
    })
}

/// `bind` over text that may be absent.
pub(crate) fn text(value: Option<&str>) -> Sql {
    value.map_or(Sql::Null, |text| Sql::Text(text.to_owned()))
}

/// Python's `round`, which rounds half to even.
fn round(value: &Value) -> Result<Sql, Failure> {
    match value {
        Value::Bool(value) => Ok(Sql::Integer(i64::from(*value))),
        Value::Number { raw, .. } if is_float(raw) => {
            let number = raw.parse::<f64>().ok().filter(|number| number.is_finite()).ok_or_else(|| unbindable(raw))?;
            Ok(Sql::Integer(number.round_ties_even() as i64))
        }
        Value::Number { .. } => bind(Some(value)),
        other => Err(Failure::Refused(format!("an ingest row's ETA is not a number: {}", pyjson::dumps(other)))),
    }
}

fn unbindable(text: &str) -> Failure {
    Failure::Refused(format!("an ingest row holds a value sqlite cannot store: {text}"))
}

/// A ledger value as `json.loads` would have read it.
pub(crate) fn value(sql: &Sql) -> Value {
    match sql {
        Sql::Null | Sql::Blob(_) => Value::Null,
        Sql::Integer(number) => integer(*number),
        Sql::Real(number) => Value::float(*number),
        Sql::Text(text) => Value::from(text.as_str()),
    }
}

/// Python's `repr` of a JSON value, for the refusal that names it.
pub(crate) fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let text = text.lossy();
            let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
            let escaped: String = text.chars().map(|c| escape(c, quote)).collect();
            format!("{quote}{escaped}{quote}")
        }
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Null => "None".into(),
        other => pyjson::dumps(other),
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
