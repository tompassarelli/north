use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Number;
use std::fmt;

#[derive(Debug, PartialEq)]
pub(super) enum Json {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
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

    fn visit_str<E>(self, value: &str) -> Result<Json, E> {
        Ok(Json::String(value.to_owned()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = map.next_entry()? {
            entries.push(entry);
        }
        Ok(Json::Object(entries))
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

impl Json {
    pub(super) fn object<const N: usize>(entries: [(&str, Json); N]) -> Json {
        Json::Object(entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
    }

    pub(super) fn render(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Number(value) => out.push_str(&value.to_string()),
            Json::String(value) => out.push_str(&north_base::json_text::ascii_string_escaping_del(value)),
            Json::Array(items) if items.is_empty() => out.push_str("[]"),
            Json::Object(entries) if entries.is_empty() => out.push_str("{}"),
            Json::Array(items) => {
                Self::members(out, depth, ('[', ']'), items.iter().map(|item| (None, item)));
            }
            Json::Object(entries) => {
                let members = entries.iter().map(|(key, value)| (Some(key.as_str()), value));
                Self::members(out, depth, ('{', '}'), members);
            }
        }
    }

    fn members<'a>(
        out: &mut String,
        depth: usize,
        (open, close): (char, char),
        members: impl Iterator<Item = (Option<&'a str>, &'a Json)>,
    ) {
        out.push(open);
        for (index, (key, value)) in members.enumerate() {
            out.push_str(if index == 0 { "\n" } else { ",\n" });
            out.push_str(&"  ".repeat(depth + 1));
            if let Some(key) = key {
                out.push_str(&north_base::json_text::ascii_string_escaping_del(key));
                out.push_str(": ");
            }
            value.write(out, depth + 1);
        }
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
        out.push(close);
    }
}
