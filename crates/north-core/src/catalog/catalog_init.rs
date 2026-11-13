use serde_json::{Map, Value, json};

use crate::error::{NorthError, NorthResult};

use super::owner_modules::STRUCTURE_FILE;

pub fn initial_catalog(structure: &str) -> NorthResult<Value> {
    let document = structure
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| NorthError::Configuration(format!("{STRUCTURE_FILE} does not parse: {error}")))?;
    let ids: Vec<&str> = document
        .get("registered")
        .and_then(|entries| entries.as_array_of_tables())
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("id").and_then(|id| id.as_str()))
        .filter(|id| id.starts_with("skill."))
        .collect();
    let registrations: Map<String, Value> = ids
        .iter()
        .map(|id| ((*id).to_owned(), json!({"kind": "skill", "category": "uncategorized", "owner": {"repo": "north", "module": id}})))
        .collect();
    let activation: Map<String, Value> = ids
        .iter()
        .map(|id| ((*id).to_owned(), json!({"distributions": [{"type": "skill", "targets": ["shared"]}]})))
        .collect();
    Ok(json!({
        "$schema": "north:agent-catalog/catalog-config.schema.json",
        "schema": "north.agent-catalog-config/v1",
        "role": "operator",
        "rootOrder": [],
        "baselines": [{"owner": {"repo": "south", "path": "agents/AGENTS.md"}, "targets": ["shared"]}],
        "providerSupport": [],
        "registrations": registrations,
        "activation": activation,
    }))
}
