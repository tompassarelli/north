use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};

use super::paths::repo_root;
use super::{configuration, string_field};

pub(super) const STRUCTURE_FILE: &str = "structure.toml";

pub fn read_resolved_catalog(path: &Path) -> NorthResult<Value> {
    let mut catalog: Value = serde_json::from_slice(&fs::read(path)?)?;
    resolve_owner_modules(&mut catalog, &mut |repo| {
        let structure = repo_root(repo)?.join(STRUCTURE_FILE);
        fs::read_to_string(&structure).map_err(|error| {
            NorthError::Configuration(format!("cannot read {}: {error}", structure.display()))
        })
    })?;
    Ok(catalog)
}

pub(super) fn resolve_owner_modules(
    value: &mut Value,
    structure_of: &mut dyn FnMut(&str) -> NorthResult<String>,
) -> NorthResult<()> {
    resolve(value, structure_of, &mut BTreeMap::new())
}

fn resolve(
    value: &mut Value,
    structure_of: &mut dyn FnMut(&str) -> NorthResult<String>,
    structures: &mut BTreeMap<String, String>,
) -> NorthResult<()> {
    match value {
        Value::Object(object) => {
            if let Some(owner) = object
                .get_mut("owner")
                .filter(|owner| owner.get("module").is_some())
            {
                let repo = string_field(owner, "repo")?.to_owned();
                let module = string_field(owner, "module")?.to_owned();
                if owner.get("path").is_some() {
                    return configuration(format!(
                        "owner {repo}:{module} names both a module and a path"
                    ));
                }
                if !structures.contains_key(&repo) {
                    structures.insert(repo.clone(), structure_of(&repo)?);
                }
                let path = registered_path(&structures[&repo], &module)
                    .map_err(|error| NorthError::Configuration(format!("{repo}: {error}")))?;
                let fields = owner
                    .as_object_mut()
                    .expect("owner with a module is an object");
                fields.remove("module");
                fields.insert("path".into(), Value::String(path));
            }
            object
                .values_mut()
                .try_for_each(|child| resolve(child, structure_of, structures))
        }
        Value::Array(items) => items
            .iter_mut()
            .try_for_each(|child| resolve(child, structure_of, structures)),
        _ => Ok(()),
    }
}

pub(super) fn registered_path(structure: &str, module: &str) -> Result<String, String> {
    let document = structure
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("{STRUCTURE_FILE} does not parse: {error}"))?;
    let mut paths = document
        .get("registered")
        .and_then(|entries| entries.as_array_of_tables())
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("id").and_then(|id| id.as_str()) == Some(module))
        .map(|entry| entry.get("path").and_then(|path| path.as_str()));
    match (paths.next(), paths.next()) {
        (Some(Some(path)), None) => Ok(path.to_owned()),
        (Some(_), Some(_)) => Err(format!("{STRUCTURE_FILE} registers {module} more than once")),
        (Some(None), None) => Err(format!("{STRUCTURE_FILE} registers {module} without a path")),
        (None, _) => Err(format!("{STRUCTURE_FILE} does not register {module}")),
    }
}

pub(super) fn collect_owner_paths(value: &Value, owners: &mut Vec<(String, String)>) -> NorthResult<()> {
    match value {
        Value::Object(object) => {
            if let Some(owner) = object.get("owner") {
                let repo = string_field(owner, "repo")?;
                let path = string_field(owner, "path")?;
                if !valid_owner_repo(repo) || !valid_owner_path(path) {
                    return configuration(format!("unsafe registered path {repo}:{path}"));
                }
                owners.push((repo.to_owned(), path.to_owned()));
            }
            for child in object.values() {
                collect_owner_paths(child, owners)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_owner_paths(child, owners)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn valid_owner_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !path
            .components()
            .any(|component| component.as_os_str() == "..")
}

pub(super) fn valid_owner_repo(repo: &str) -> bool {
    !repo.is_empty()
        && !repo.contains("..")
        && repo
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

