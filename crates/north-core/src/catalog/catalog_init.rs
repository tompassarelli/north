use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::error::{NorthError, NorthResult};

use super::guard_ids::{guard_ids_asked, scan_sources};
use super::owner_modules::STRUCTURE_FILE;

pub fn initial_catalog(north: &Path) -> NorthResult<Value> {
    let structure = fs::read_to_string(north.join(STRUCTURE_FILE))?;
    let guard_sources = scan_sources(north)?;
    catalog_from(&structure, &guard_sources)
}

pub(super) fn catalog_from(structure: &str, guard_sources: &str) -> NorthResult<Value> {
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
    let mut activation: Map<String, Value> = ids
        .iter()
        .map(|id| ((*id).to_owned(), json!({"distributions": [{"type": "skill", "targets": ["shared"]}]})))
        .collect();
    activation.extend(guard_ids_asked(guard_sources, "").into_iter().map(|id| (id, json!({"distributions": []}))));
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
