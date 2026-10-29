use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::error::{NorthError, NorthResult};

use super::digest::provenance;
use super::files::{read_current, read_json};
use super::generation::publish;
use super::paths::owner_path;
use super::reviews::{check_reviews, skill_metadata};
use super::{configuration, find_unit_optional, human_title, object_mut, owner, string_field, units, units_mut};

pub(super) fn project_package_registrations(overlays: &Map<String, Value>) -> NorthResult<Map<String, Value>> {
    let mut catalogs = BTreeSet::new();
    let mut registrations = Map::new();
    for overlay in overlays.values() {
        for distribution in overlay
            .get("distributions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if distribution.get("type").and_then(Value::as_str) != Some("projectPackage") {
                continue;
            }
            let catalog_owner = owner(distribution)?.clone();
            let key = (
                string_field(&catalog_owner, "repo")?.to_owned(),
                string_field(&catalog_owner, "path")?.to_owned(),
            );
            if !catalogs.insert(key) {
                continue;
            }
            let catalog = read_json(&owner_path(&catalog_owner)?)?;
            let units = catalog
                .get("units")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    NorthError::Configuration("project package units are missing".into())
                })?;
            for unit in units {
                let id = string_field(unit, "id")?;
                let registration = catalog_registration(unit, &catalog_owner)?;
                if registrations.insert(id.into(), registration).is_some() {
                    return configuration(format!(
                        "unit {id} is registered by multiple project packages"
                    ));
                }
            }
        }
    }
    Ok(registrations)
}

pub(super) fn catalog_registration(unit: &Value, catalog_owner: &Value) -> NorthResult<Value> {
    let catalog_path = Path::new(string_field(catalog_owner, "path")?);
    let base = catalog_path.parent().unwrap_or(Path::new(""));
    let source = Path::new(string_field(unit, "source")?);
    if source.is_absolute()
        || source
            .components()
            .any(|component| component.as_os_str() == "..")
    {
        return configuration("project package source escapes its repository");
    }
    Ok(json!({
        "kind": string_field(unit, "kind")?,
        "category": unit.get("category").cloned().unwrap_or(Value::Null),
        "title": unit.get("title").cloned().unwrap_or(Value::Null),
        "triggerDescription": unit.get("triggerDescription").cloned().unwrap_or(Value::Null),
        "members": unit.get("members").cloned().unwrap_or_else(|| json!([])),
        "owner": {
            "repo": string_field(catalog_owner, "repo")?,
            "path": base.join(source).display().to_string(),
        },
    }))
}

pub(super) fn change_permission(id: &str, permission: &str) -> NorthResult<Value> {
    let mut activation = read_current()?;
    if find_unit_optional(&activation, id).is_none() {
        return configuration(format!("unknown unit: {id}; run `agents sync` first"));
    }
    activation
        .get_mut("permissions")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            NorthError::Configuration("current activation permissions are missing".into())
        })?
        .insert(id.into(), Value::String(permission.into()));

    recompute_activation(&mut activation)?;
    rebuild_projection_plan(&mut activation)?;
    publish(activation)
}

pub(super) fn new_unit(id: &str, registration: &Value, overlay: &Value) -> NorthResult<Value> {
    let owner = registration
        .get("owner")
        .ok_or_else(|| NorthError::Configuration(format!("registration {id} has no owner")))?
        .clone();
    let kind = string_field(registration, "kind")?;
    let source = owner_path(&owner)?;
    let metadata = if kind == "skill" {
        let metadata = skill_metadata(&source)?;
        check_reviews(&metadata, &source)?;
        metadata
    } else {
        BTreeMap::new()
    };
    let title = registration
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| human_title(id));
    let trigger = registration
        .get("triggerDescription")
        .and_then(Value::as_str)
        .or_else(|| metadata.get("description").map(String::as_str))
        .unwrap_or("Activate this unit.");
    let distributions = overlay
        .get("distributions")
        .and_then(Value::as_array)
        .ok_or_else(|| NorthError::Configuration(format!("unit {id} has no distributions")))?
        .iter()
        .map(|distribution| {
            let mut distribution = enrich_distribution(id, kind, &owner, distribution)?;
            if kind == "skill" && distribution.get("type").and_then(Value::as_str) == Some("skill")
            {
                restrict_skill_agents(&mut distribution, &metadata)?;
            }
            Ok(distribution)
        })
        .collect::<NorthResult<Vec<_>>>()?;
    Ok(json!({
        "id": id,
        "kind": kind,
        "title": title,
        "triggerDescription": trigger,
        "category": registration.get("category").cloned().unwrap_or(Value::Null),
        "owner": owner,
        "members": registration.get("members").cloned().unwrap_or_else(|| json!([])),
        "supports": overlay.get("supports").cloned().unwrap_or_else(|| json!([])),
        "distributions": distributions,
        "ownerProvenance": provenance(registration.get("owner").expect("owner checked"))?,
        "permission": "off",
        "active": false,
        "activationPaths": [],
    }))
}

pub(super) fn restrict_skill_agents(
    distribution: &mut Value,
    metadata: &BTreeMap<String, String>,
) -> NorthResult<()> {
    let Some(raw) = metadata.get("agents") else {
        return Ok(());
    };
    let list = raw
        .strip_prefix('[')
        .and_then(|list| list.strip_suffix(']'))
        .ok_or_else(|| NorthError::Configuration("skill agents must be an inline list".into()))?;
    let agents = list
        .split(',')
        .map(|agent| agent.trim().trim_matches(['\'', '"']))
        .filter(|agent| !agent.is_empty())
        .collect::<BTreeSet<_>>();
    if agents
        .iter()
        .any(|agent| !["claude", "codex"].contains(agent))
    {
        return configuration("skill agents must name claude or codex");
    }
    let targets = distribution
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| NorthError::Configuration("skill targets are missing".into()))?;
    let targets = agents
        .into_iter()
        .filter(|agent| {
            targets
                .iter()
                .any(|target| target.as_str() == Some("shared") || target.as_str() == Some(*agent))
        })
        .collect::<Vec<_>>();
    object_mut(distribution)?.insert("targets".into(), json!(targets));
    Ok(())
}

fn enrich_distribution(
    id: &str,
    kind: &str,
    owner: &Value,
    distribution: &Value,
) -> NorthResult<Value> {
    let distribution_type = string_field(distribution, "type")?;
    let distribution_owner = distribution.get("owner").cloned().unwrap_or_else(|| {
        if kind == "skill" && distribution_type == "skill" {
            let mut owner = owner.clone();
            if let Some(path) = owner.get("path").and_then(Value::as_str).map(str::to_owned) {
                let directory = Path::new(&path)
                    .parent()
                    .unwrap_or(Path::new(&path))
                    .to_path_buf();
                if let Some(object) = owner.as_object_mut() {
                    object.insert(
                        "path".into(),
                        Value::String(directory.display().to_string()),
                    );
                }
            }
            owner
        } else {
            owner.clone()
        }
    });
    Ok(json!({
        "type": distribution_type,
        "targets": distribution.get("targets").cloned().unwrap_or_else(|| json!([])),
        "owner": distribution_owner,
        "adapterId": distribution.get("adapterId").and_then(Value::as_str).unwrap_or(id),
        "provenance": provenance(&distribution_owner)?,
    }))
}

pub(super) fn refresh_provenance(activation: &mut Value) -> NorthResult<()> {
    for unit in units_mut(activation)? {
        let owner_value = owner(unit)?.clone();
        object_mut(unit)?.insert("ownerProvenance".into(), provenance(&owner_value)?);
        if let Some(distributions) = unit.get_mut("distributions").and_then(Value::as_array_mut) {
            for distribution in distributions {
                let distribution_owner = owner(distribution)?.clone();
                object_mut(distribution)?
                    .insert("provenance".into(), provenance(&distribution_owner)?);
            }
        }
    }
    for field in ["baselines", "providerSupport"] {
        let entries = activation
            .get_mut(field)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                NorthError::Configuration(format!("activation {field} must be an array"))
            })?;
        for entry in entries {
            let owner_value = owner(entry)?.clone();
            object_mut(entry)?.insert("provenance".into(), provenance(&owner_value)?);
        }
    }
    Ok(())
}

pub(super) fn ensure_permissions(activation: &mut Value) -> NorthResult<()> {
    let ids = units(activation)?
        .iter()
        .map(|unit| string_field(unit, "id").map(str::to_owned))
        .collect::<NorthResult<Vec<_>>>()?;
    let previous = activation
        .get_mut("permissions")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            NorthError::Configuration("current activation permissions are missing".into())
        })?
        .clone();
    let mut permissions = Map::new();
    for id in ids {
        permissions.insert(
            id.clone(),
            previous
                .get(&id)
                .cloned()
                .unwrap_or_else(|| Value::String("off".into())),
        );
    }
    object_mut(activation)?.insert("permissions".into(), Value::Object(permissions));
    Ok(())
}

pub(super) fn recompute_activation(activation: &mut Value) -> NorthResult<()> {
    let permissions = activation
        .get("permissions")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            NorthError::Configuration("current activation permissions are missing".into())
        })?
        .clone();
    let roots = activation
        .get("rootOrder")
        .and_then(Value::as_array)
        .ok_or_else(|| NorthError::Configuration("activation root order is missing".into()))?
        .iter()
        .map(|root| {
            root.as_str()
                .map(str::to_owned)
                .ok_or_else(|| NorthError::Configuration("activation root is not a string".into()))
        })
        .collect::<NorthResult<Vec<_>>>()?;

    let snapshot = units(activation)?.clone();
    let known = snapshot
        .iter()
        .map(|unit| string_field(unit, "id").map(str::to_owned))
        .collect::<NorthResult<BTreeSet<_>>>()?;
    let mut edges = BTreeMap::<String, Vec<String>>::new();
    for unit in &snapshot {
        let id = string_field(unit, "id")?;
        for member in unit
            .get("members")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let member = member.as_str().ok_or_else(|| {
                NorthError::Configuration(format!("unit {id} has a non-string member"))
            })?;
            if !known.contains(member) {
                return configuration(format!("unit {id} names unknown member {member}"));
            }
            edges.entry(id.into()).or_default().push(member.into());
        }
        for supported in unit
            .get("supports")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let supported = supported.as_str().ok_or_else(|| {
                NorthError::Configuration(format!("unit {id} has a non-string support"))
            })?;
            if !known.contains(supported) {
                return configuration(format!("unit {id} supports unknown unit {supported}"));
            }
            edges.entry(supported.into()).or_default().push(id.into());
        }
    }

    let permission_on = |id: &str| permissions.get(id).and_then(Value::as_str) == Some("on");
    let mut queue = VecDeque::new();
    for root in roots {
        if known.contains(&root) && permission_on(&root) {
            queue.push_back((root.clone(), vec![root]));
        }
    }
    let mut paths = BTreeMap::<String, Vec<Vec<String>>>::new();
    while let Some((id, path)) = queue.pop_front() {
        let unit_paths = paths.entry(id.clone()).or_default();
        if unit_paths.contains(&path) {
            continue;
        }
        unit_paths.push(path.clone());
        for next in edges.get(&id).into_iter().flatten() {
            if permission_on(next) && !path.contains(next) {
                let mut next_path = path.clone();
                next_path.push(next.clone());
                queue.push_back((next.clone(), next_path));
            }
        }
    }

    for unit in units_mut(activation)? {
        let id = string_field(unit, "id")?.to_owned();
        let permission = permissions
            .get(&id)
            .and_then(Value::as_str)
            .unwrap_or("off");
        let activation_paths = paths.remove(&id).unwrap_or_default();
        object_mut(unit)?.insert("permission".into(), Value::String(permission.into()));
        object_mut(unit)?.insert("active".into(), Value::Bool(!activation_paths.is_empty()));
        object_mut(unit)?.insert("activationPaths".into(), json!(activation_paths));
    }
    Ok(())
}

pub(super) fn rebuild_projection_plan(activation: &mut Value) -> NorthResult<()> {
    let mut plan = Map::new();
    for unit in units(activation)? {
        let active = unit.get("active").and_then(Value::as_bool).unwrap_or(false);
        let id = string_field(unit, "id")?;
        let distributions = unit
            .get("distributions")
            .and_then(Value::as_array)
            .ok_or_else(|| NorthError::Configuration(format!("unit {id} has no distributions")))?;
        for distribution in distributions {
            let distribution_type = string_field(distribution, "type")?;
            if !active && distribution_type != "hook" && distribution_type != "providerAdapter" {
                continue;
            }
            let targets = distribution
                .get("targets")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    NorthError::Configuration(format!("unit {id} has invalid targets"))
                })?;
            for target in targets {
                let target = target.as_str().ok_or_else(|| {
                    NorthError::Configuration(format!("unit {id} has a non-string target"))
                })?;
                let types = plan
                    .entry(distribution_type)
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .ok_or_else(|| NorthError::Configuration("invalid projection plan".into()))?;
                let entries = types
                    .entry(target)
                    .or_insert_with(|| Value::Array(Vec::new()))
                    .as_array_mut()
                    .ok_or_else(|| NorthError::Configuration("invalid projection target".into()))?;
                entries.push(json!({
                    "unitId": id,
                    "owner": distribution.get("owner").cloned().unwrap_or(Value::Null),
                    "adapterId": distribution.get("adapterId").cloned().unwrap_or(Value::Null),
                    "provenance": distribution.get("provenance").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }
    object_mut(activation)?.insert("projectionPlan".into(), Value::Object(plan));
    Ok(())
}
