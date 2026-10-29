use std::collections::BTreeMap;

#[path = "catalog/activation.rs"]
mod activation;
#[path = "catalog/digest.rs"]
mod digest;
#[path = "catalog/files.rs"]
mod files;
#[path = "catalog/generation.rs"]
mod generation;
#[path = "catalog/paths.rs"]
mod paths;
#[path = "catalog/registration_checks.rs"]
mod registration_checks;
#[path = "catalog/reviews.rs"]
mod reviews;
#[path = "catalog/sync_gate.rs"]
pub(crate) mod sync_gate;
#[cfg(test)]
#[path = "catalog/sync_gate_tests.rs"]
mod sync_gate_tests;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};

use activation::{
    change_permission, ensure_permissions, new_unit, project_package_registrations, rebuild_projection_plan,
    recompute_activation, refresh_provenance,
};
use digest::catalog_digest;
use files::{read_current, read_json, with_lock};
use generation::publish;
use paths::{agents_root, home, operator_catalog_path, operator_dir, owner_path, repo_root};
use reviews::{print_stale, stale_skills};

const ACTIVATION_SCHEMA: &str = "north.agent-activation/v1";
const MANAGED_SKILLS_SCHEMA: &str = "north.codex-managed-skills/v1";
const REVIEW_STALE_DAYS: i64 = 30;

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let (verb, rest) = arguments
        .split_first()
        .map_or(("status", &[][..]), |(verb, rest)| (verb.as_str(), rest));
    match verb {
        "sync" => {
            let json = only_json(rest)?;
            let activation = with_lock(sync)?;
            if json {
                print_json(&activation)?;
            } else {
                println!(
                    "agents synchronized → {}/current ({}/{}) active)",
                    agents_root()?.display(),
                    active_count(&activation),
                    units(&activation)?.len()
                );
                print_stale(&stale_skills(&activation)?);
            }
        }
        "check-hook-paths" => {
            let inputs = registration_checks::HookPathInputs::parse(rest)?;
            let report = registration_checks::validate_hook_registrations(&inputs)?;
            println!(
                "Claude hook registrations: {}, missing paths: 0",
                report.claude
            );
            println!(
                "Codex hook registrations: {}, missing paths: 0",
                report.codex
            );
            println!(
                "Codex runtime paths: {}, missing paths: 0",
                report.codex_runtime
            );
        }
        "check-owners" => registration_checks::check_registered_owners(rest)?,
        "stale" => {
            let json = only_json(rest)?;
            let stale = stale_skills(&read_current()?)?;
            if json {
                print_json(&Value::Array(stale))?;
            } else {
                print_stale(&stale);
            }
        }
        "on" | "off" => {
            let (id, json) = id_and_json(rest)?;
            let permission = verb.to_owned();
            let activation = with_lock(|| change_permission(id, &permission))?;
            if json {
                print_json(&activation)?;
            } else {
                println!(
                    "{id} → {verb} · generation {}",
                    string_field(&activation, "generationId")?
                );
            }
        }
        "status" => {
            let json = only_json(rest)?;
            output_activation(&read_current()?, json)?;
        }
        "path" => {
            let (id, json) = id_and_json(rest)?;
            let activation = read_current()?;
            let unit = find_unit(&activation, id)?;
            let path = owner_path(owner(unit)?)?;
            if json {
                print_json(&json!({
                    "id": id,
                    "kind": string_field(unit, "kind")?,
                    "owner": owner(unit)?,
                    "path": path,
                }))?;
            } else {
                println!("{}", path.display());
            }
        }
        "inspect" => {
            let (id, json) = id_and_json(rest)?;
            let activation = read_current()?;
            let mut unit = find_unit(&activation, id)?.clone();
            let resolved = owner_path(owner(&unit)?)?;
            object_mut(&mut unit)?.insert(
                "resolvedOwnerPath".into(),
                Value::String(resolved.display().to_string()),
            );
            if json {
                print_json(&unit)?;
            } else {
                print_unit(&unit)?;
                println!("  trigger: {}", string_field(&unit, "triggerDescription")?);
                println!(
                    "  active via: {}",
                    unit.get("activationPaths")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "[]".into())
                );
            }
        }
        _ => return configuration(usage()),
    }
    Ok(())
}

fn sync() -> NorthResult<Value> {
    let mut activation = read_current()?;
    let operator = read_json(&operator_catalog_path()?)?;
    let operator_registrations = operator
        .get("registrations")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            NorthError::Configuration("operator catalog registrations are missing".into())
        })?;
    let overlays = operator
        .get("activation")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            NorthError::Configuration("operator catalog activation is missing".into())
        })?;

    let mut registrations = project_package_registrations(overlays)?;
    for (id, registration) in operator_registrations {
        if registrations
            .insert(id.clone(), registration.clone())
            .is_some()
        {
            return configuration(format!("unit {id} has more than one registration"));
        }
    }
    let skipped = skip_skills_without_checkout(&mut registrations, |repo| {
        repo_root(repo).map(|root| root.is_dir())
    })?;
    for (id, repo) in &skipped {
        eprintln!("agents sync: skipping skill {id}: owner repository {repo} has no checkout");
    }

    let previous = units(&activation)?
        .iter()
        .filter_map(|unit| {
            unit.get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_owned(), unit.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut refreshed = Vec::with_capacity(registrations.len());
    for (id, registration) in &registrations {
        let overlay = overlays.get(id).ok_or_else(|| {
            NorthError::Configuration(format!("operator activation omits unit {id}"))
        })?;
        let mut unit = new_unit(id, registration, overlay)?;
        for field in ["members", "supports"] {
            if let Some(list) = unit.get_mut(field).and_then(Value::as_array_mut) {
                list.retain(|entry| {
                    entry
                        .as_str()
                        .is_none_or(|entry| !skipped.contains_key(entry))
                });
            }
        }
        if let Some(old) = previous.get(id) {
            for field in ["permission", "active", "activationPaths"] {
                if let Some(value) = old.get(field) {
                    object_mut(&mut unit)?.insert(field.into(), value.clone());
                }
            }
        }
        refreshed.push(unit);
    }
    object_mut(&mut activation)?.insert("units".into(), Value::Array(refreshed));

    if let Some(root_order) = operator.get("rootOrder") {
        object_mut(&mut activation)?.insert("rootOrder".into(), root_order.clone());
    }
    for field in ["baselines", "providerSupport"] {
        let value = operator.get(field).cloned().ok_or_else(|| {
            NorthError::Configuration(format!("operator catalog {field} is missing"))
        })?;
        if !value.is_array() {
            return configuration(format!("operator catalog {field} must be an array"));
        }
        object_mut(&mut activation)?.insert(field.into(), value);
    }
    refresh_provenance(&mut activation)?;
    ensure_permissions(&mut activation)?;
    recompute_activation(&mut activation)?;
    rebuild_projection_plan(&mut activation)?;
    let digest = catalog_digest()?;
    object_mut(&mut activation)?.insert("catalogDigest".into(), Value::String(digest));
    publish(activation)
}

fn skip_skills_without_checkout(
    registrations: &mut Map<String, Value>,
    checked_out: impl Fn(&str) -> NorthResult<bool>,
) -> NorthResult<BTreeMap<String, String>> {
    let mut skipped = BTreeMap::new();
    for (id, registration) in registrations.iter() {
        let repo = string_field(owner(registration)?, "repo")?;
        if checked_out(repo)? {
            continue;
        }
        if string_field(registration, "kind")? != "skill" {
            return configuration(format!(
                "unit {id} needs a checkout of its owner repository {repo}"
            ));
        }
        skipped.insert(id.clone(), repo.to_owned());
    }
    for id in skipped.keys() {
        registrations.remove(id);
    }
    Ok(skipped)
}

fn human_title(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + characters.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn print_json(value: &Value) -> NorthResult<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn output_activation(activation: &Value, json: bool) -> NorthResult<()> {
    if json {
        return print_json(activation);
    }
    println!("generation: {}", string_field(activation, "generationId")?);
    println!("catalog:    {}", string_field(activation, "catalogDigest")?);
    for unit in units(activation)? {
        print_unit(unit)?;
    }
    Ok(())
}

fn print_unit(unit: &Value) -> NorthResult<()> {
    let owner = owner(unit)?;
    println!(
        "{:<34} {:<5} {:<27} {}:{}",
        string_field(unit, "id")?,
        string_field(unit, "kind")?,
        format!(
            "{} · {}",
            string_field(unit, "permission")?,
            if unit.get("active").and_then(Value::as_bool).unwrap_or(false) {
                "active"
            } else {
                "inactive"
            }
        ),
        string_field(owner, "repo")?,
        string_field(owner, "path")?,
    );
    Ok(())
}

fn only_json(arguments: &[String]) -> NorthResult<bool> {
    match arguments {
        [] => Ok(false),
        [argument] if argument == "--json" => Ok(true),
        _ => configuration(usage()),
    }
}

fn id_and_json(arguments: &[String]) -> NorthResult<(&str, bool)> {
    match arguments {
        [id] => Ok((id, false)),
        [id, argument] if argument == "--json" => Ok((id, true)),
        _ => configuration(usage()),
    }
}

fn active_count(activation: &Value) -> usize {
    units(activation)
        .map(|units| {
            units
                .iter()
                .filter(|unit| unit.get("active").and_then(Value::as_bool).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

fn units(activation: &Value) -> NorthResult<&Vec<Value>> {
    activation
        .get("units")
        .and_then(Value::as_array)
        .ok_or_else(|| NorthError::Configuration("activation units are missing".into()))
}

fn units_mut(activation: &mut Value) -> NorthResult<&mut Vec<Value>> {
    activation
        .get_mut("units")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| NorthError::Configuration("activation units are missing".into()))
}

fn find_unit<'a>(activation: &'a Value, id: &str) -> NorthResult<&'a Value> {
    find_unit_optional(activation, id)
        .ok_or_else(|| NorthError::Configuration(format!("unknown unit: {id}")))
}

fn find_unit_optional<'a>(activation: &'a Value, id: &str) -> Option<&'a Value> {
    activation
        .get("units")?
        .as_array()?
        .iter()
        .find(|unit| unit.get("id").and_then(Value::as_str) == Some(id))
}

fn owner(value: &Value) -> NorthResult<&Value> {
    value
        .get("owner")
        .ok_or_else(|| NorthError::Configuration("owner is missing".into()))
}

fn object_mut(value: &mut Value) -> NorthResult<&mut Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| NorthError::Configuration("expected an object".into()))
}

fn string_field<'a>(value: &'a Value, field: &str) -> NorthResult<&'a str> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        NorthError::Configuration(format!("required string field is missing: {field}"))
    })
}

fn configuration<T>(message: impl Into<String>) -> NorthResult<T> {
    Err(NorthError::Configuration(message.into()))
}

fn usage() -> String {
    "usage: north config agents [sync|stale|status|on|off|path|inspect|check-hook-paths|check-owners] ...".into()
}

#[cfg(test)]
#[path = "catalog/agent_catalog_tests.rs"]
mod tests;
