use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Number;

use super::pyfmt::{float_repr, json_string};

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

impl Json {
    pub fn field(&self, key: &str) -> &Json {
        match self {
            Json::Object(entries) => entries.iter().find(|(name, _)| name == key).map_or(&NULL, |(_, value)| value),
            _ => &NULL,
        }
    }

    pub fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            _ => &[],
        }
    }

    pub fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Object(entries) => entries,
            _ => &[],
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.field(key).as_str()
    }

    pub fn nonempty(&self, key: &str) -> Option<&str> {
        self.text(key).filter(|text| !text.is_empty())
    }

    pub fn is_type(&self, name: &str) -> bool {
        self.text("type") == Some(name)
    }

    pub fn number(&self) -> f64 {
        match self {
            Json::Number(number) => number.as_f64().unwrap_or(0.0),
            Json::Bool(flag) => f64::from(u8::from(*flag)),
            _ => 0.0,
        }
    }

    pub fn integer(&self) -> i64 {
        match self {
            Json::Number(number) => number.as_i64().unwrap_or_else(|| number.as_f64().unwrap_or(0.0) as i64),
            Json::Bool(flag) => i64::from(*flag),
            _ => 0,
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(flag) => *flag,
            Json::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
            Json::String(text) => !text.is_empty(),
            Json::Array(items) => !items.is_empty(),
            Json::Object(entries) => !entries.is_empty(),
        }
    }

    pub fn dumps(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(flag) => flag.to_string(),
            Json::Number(number) if number.is_f64() => float_repr(number.as_f64().unwrap_or(0.0)),
            Json::Number(number) => number.to_string(),
            Json::String(text) => json_string(text),
            Json::Array(items) => format!("[{}]", items.iter().map(Json::dumps).collect::<Vec<_>>().join(", ")),
            Json::Object(entries) => format!(
                "{{{}}}",
                entries.iter().map(|(key, value)| format!("{}: {}", json_string(key), value.dumps())).collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E>(self, flag: bool) -> Result<Json, E> {
        Ok(Json::Bool(flag))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Json, E> {
        Ok(Json::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Json, E> {
        Ok(Json::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Json, E> {
        Ok(Number::from_f64(value).map_or(Json::Null, Json::Number))
    }

    fn visit_str<E>(self, text: &str) -> Result<Json, E> {
        Ok(Json::String(text.to_owned()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut entries: Vec<(String, Json)> = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            match entries.iter_mut().find(|(name, _)| *name == key) {
                Some(slot) => slot.1 = value,
                None => entries.push((key, value)),
            }
        }
        Ok(Json::Object(entries))
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}
