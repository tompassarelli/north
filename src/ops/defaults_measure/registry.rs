use serde_json::Value;
use toml_edit::{DocumentMut, Item};

const BUNDLED: &str = include_str!("../../../harness/defaults.toml");

pub(super) struct Entry {
    pub(super) name: String,
    pub(super) value: Value,
    pub(super) source: String,
    pub(super) settles: Option<Settles>,
}

pub(super) struct Settles {
    pub(super) count: String,
    pub(super) need: i64,
}

pub(super) struct Registry(Vec<Entry>);

impl Registry {
    pub(super) fn bundled() -> Self {
        Self::parse(BUNDLED).expect("harness/defaults.toml parses")
    }

    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let document: DocumentMut = text.parse().map_err(|error| format!("harness/defaults.toml: {error}"))?;
        let entries = document
            .iter()
            .map(|(name, item)| Entry {
                name: name.to_owned(),
                value: item.get("value").map_or(Value::Null, scalar),
                source: item.get("source").and_then(Item::as_str).unwrap_or_default().to_owned(),
                settles: item.get("settles_when").and_then(|settles| {
                    Some(Settles { count: settles.get("count")?.as_str()?.to_owned(), need: settles.get("need")?.as_integer()? })
                }),
            })
            .collect();
        Ok(Self(entries))
    }

    pub(super) fn entries(&self) -> &[Entry] {
        &self.0
    }

    pub(super) fn value(&self, name: &str) -> &Value {
        self.0.iter().find(|entry| entry.name == name).map_or(&Value::Null, |entry| &entry.value)
    }

    pub(super) fn number(&self, name: &str) -> f64 {
        self.value(name).as_f64().unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
    }
}

fn scalar(item: &Item) -> Value {
    item.as_value().map_or(Value::Null, json)
}

fn json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::Integer(value) => (*value.value()).into(),
        toml_edit::Value::Float(value) => serde_json::Number::from_f64(*value.value()).map_or(Value::Null, Value::Number),
        toml_edit::Value::String(value) => value.value().clone().into(),
        toml_edit::Value::Boolean(value) => (*value.value()).into(),
        toml_edit::Value::Array(values) => values.iter().map(json).collect(),
        _ => Value::Null,
    }
}
