use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};
use toml_edit::DocumentMut;

use crate::router::policy::json_value;

use super::super::tiers::{self, Tier};

pub(super) struct Config {
    pub document: DocumentMut,
    pub tiers: Vec<Tier>,
    value: Value,
}

pub(super) fn read_document(path: &Path) -> Result<DocumentMut, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    text.parse::<DocumentMut>().map_err(|error| format!("cannot read {}: {error}", path.display()))
}

impl Config {
    pub(super) fn read(path: &Path) -> Result<Self, String> {
        let document = read_document(path)?;
        let tiers = tiers::read(&document)?;
        let value = json_value(document.as_item());
        Ok(Self { document, tiers, value })
    }

    pub(super) fn mode(&self) -> Option<&str> {
        self.value.get("mode").and_then(Value::as_str)
    }

    pub(super) fn account_settings(&self, provider: &str) -> Map<String, Value> {
        self.value["accounts"].get(provider).and_then(Value::as_object).cloned().unwrap_or_default()
    }

    pub(super) fn projects(&self) -> impl Iterator<Item = (&str, Value)> {
        let table = self.document.get("projects").and_then(|projects| projects.as_table_like());
        table.into_iter().flat_map(|table| table.iter().map(|(name, item)| (name, json_value(item))))
    }

    pub(super) fn pace(&self) -> Value {
        self.value.get("pace").cloned().unwrap_or_else(|| json!({}))
    }
}
