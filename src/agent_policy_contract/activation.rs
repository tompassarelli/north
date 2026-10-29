use regex::Regex;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use super::wiring::{guards, text};
use super::{Contract, UNIT, py_list, py_value};

static DIGEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^sha256:[0-9a-f]{64}$").unwrap());
const SCHEMA: &str = "north.agent-activation/v1";
const REQUIRED_FIELDS: [&str; 11] = [
    "activationPaths",
    "active",
    "distributions",
    "id",
    "kind",
    "members",
    "owner",
    "permission",
    "supports",
    "title",
    "triggerDescription",
];

fn lexical(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal
}

fn resolve(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| lexical(path))
}

fn check_owner(contract: &mut Contract, unit_id: &str, owner: &Map<String, Value>, repo: &Path) {
    if owner.get("repo").and_then(Value::as_str) != Some("nixos-config") {
        return;
    }
    let Some(relative) = owner.get("path").and_then(Value::as_str) else {
        contract.reject(format!("NixOS-owned unit {unit_id} has invalid owner path"));
        return;
    };
    let source = resolve(&repo.join(relative));
    if !source.starts_with(repo) {
        contract.reject(format!("NixOS-owned unit {unit_id} escapes its repository"));
    }
    if !source.exists() {
        contract.reject(format!("NixOS-owned unit {unit_id} source is absent: {relative}"));
    }
}

fn check_unit(contract: &mut Contract, unit: &Map<String, Value>, repo: &Path, by_id: &mut BTreeMap<String, String>) {
    let missing: Vec<&str> = REQUIRED_FIELDS.into_iter().filter(|field| !unit.contains_key(*field)).collect();
    if !missing.is_empty() {
        let id = unit.get("id").map_or("None".into(), py_value);
        contract.reject(format!("North activation unit {id} lacks {}", py_list(missing.into_iter())));
        return;
    }
    let Some(unit_id) = unit["id"].as_str().filter(|id| UNIT.is_match(id)) else {
        contract.reject(format!("North activation has invalid unit id: {}", py_value(&unit["id"])));
        return;
    };
    if by_id.contains_key(unit_id) {
        contract.reject(format!("North activation duplicates global unit id {unit_id}"));
    }
    by_id.insert(unit_id.to_owned(), unit["kind"].as_str().unwrap_or("").to_owned());
    if !matches!(unit["kind"].as_str(), Some("skill" | "hook" | "module")) {
        contract.reject(format!(
            "North activation unit {unit_id} has invalid kind {}",
            py_value(&unit["kind"])
        ));
    }
    let permission = unit["permission"].as_str().filter(|value| matches!(*value, "on" | "off"));
    if permission.is_none() {
        contract.reject(format!("North activation unit {unit_id} has invalid permission"));
    }
    match unit["active"].as_bool() {
        None => contract.reject(format!("North activation unit {unit_id} has non-boolean activity")),
        Some(true) if permission == Some("off") => {
            contract.reject(format!("North activation unit {unit_id} is active despite off permission"));
        }
        Some(_) => {}
    }
    for field in ["members", "supports", "distributions", "activationPaths"] {
        if !unit[field].is_array() {
            contract.reject(format!("North activation unit {unit_id} has non-array {field}"));
        }
    }
    match unit["owner"].as_object().filter(|owner| owner.contains_key("repo") && owner.contains_key("path")) {
        Some(owner) => check_owner(contract, unit_id, owner, repo),
        None => contract.reject(format!("North activation unit {unit_id} has invalid owner")),
    }
}

pub(super) fn check(
    contract: &mut Contract,
    policy: &Value,
    repo: &Path,
    path: &Path,
) {
    let data = std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|body| serde_json::from_str::<Value>(&body).map_err(|error| error.to_string()));
    let data = match data {
        Ok(data) => data,
        Err(error) => {
            contract.reject(format!("North activation generation is unreadable: {}: {error}", path.display()));
            return;
        }
    };
    if text(&data, "schema") != SCHEMA {
        contract.reject(format!("North activation schema is not {SCHEMA}"));
    }
    for field in ["catalogDigest", "generationId"] {
        if !DIGEST.is_match(text(&data, field)) {
            contract.reject(format!("North activation {field} is invalid"));
        }
    }
    let Some(units) = data.get("units").and_then(Value::as_array) else {
        contract.reject("North activation units must be an array".into());
        return;
    };
    let mut by_id = BTreeMap::new();
    for unit in units {
        match unit.as_object() {
            Some(unit) => check_unit(contract, unit, repo, &mut by_id),
            None => contract.reject("North activation contains a non-object unit".into()),
        }
    }
    for guard in guards(policy) {
        let unit_id = text(guard, "unit");
        match by_id.get(unit_id) {
            None => contract.reject(format!("provider-bound hook is absent from North activation: {unit_id}")),
            Some(kind) if kind != "hook" => {
                contract.reject(format!("provider-bound unit is not a hook: {unit_id}"));
            }
            Some(_) => {}
        }
    }
}
