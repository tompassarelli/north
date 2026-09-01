use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::fs::{FlockOperation, flock};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};

const ACTIVATION_SCHEMA: &str = "north.agent-activation/v1";
const MANAGED_SKILLS_SCHEMA: &str = "north.codex-managed-skills/v1";

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
    let operator =
        read_json(&repo_root("nixos-config")?.join("dotfiles/agents/catalog-config.json"))?;
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
    refresh_provenance(&mut activation)?;
    ensure_permissions(&mut activation)?;
    rebuild_projection_plan(&mut activation)?;
    let digest = catalog_digest()?;
    object_mut(&mut activation)?.insert("catalogDigest".into(), Value::String(digest));
    publish(activation)
}

fn project_package_registrations(overlays: &Map<String, Value>) -> NorthResult<Map<String, Value>> {
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

fn catalog_registration(unit: &Value, catalog_owner: &Value) -> NorthResult<Value> {
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

fn change_permission(id: &str, permission: &str) -> NorthResult<Value> {
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

    let root = activation
        .get("rootOrder")
        .and_then(Value::as_array)
        .is_some_and(|roots| roots.iter().any(|root| root.as_str() == Some(id)));
    let unit = find_unit_mut(&mut activation, id)?;
    object_mut(unit)?.insert("permission".into(), Value::String(permission.into()));
    let active = permission == "on" && root;
    object_mut(unit)?.insert("active".into(), Value::Bool(active));
    object_mut(unit)?.insert(
        "activationPaths".into(),
        if active { json!([[id]]) } else { json!([]) },
    );
    rebuild_projection_plan(&mut activation)?;
    publish(activation)
}

fn new_unit(id: &str, registration: &Value, overlay: &Value) -> NorthResult<Value> {
    let owner = registration
        .get("owner")
        .ok_or_else(|| NorthError::Configuration(format!("registration {id} has no owner")))?
        .clone();
    let kind = string_field(registration, "kind")?;
    let source = owner_path(&owner)?;
    let metadata = if kind == "skill" {
        skill_metadata(&source)?
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
        .map(|distribution| enrich_distribution(id, kind, &owner, distribution))
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

fn refresh_provenance(activation: &mut Value) -> NorthResult<()> {
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
    Ok(())
}

fn ensure_permissions(activation: &mut Value) -> NorthResult<()> {
    let ids = units(activation)?
        .iter()
        .map(|unit| string_field(unit, "id").map(str::to_owned))
        .collect::<NorthResult<Vec<_>>>()?;
    let permissions = activation
        .get_mut("permissions")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            NorthError::Configuration("current activation permissions are missing".into())
        })?;
    for id in ids {
        permissions
            .entry(id)
            .or_insert_with(|| Value::String("off".into()));
    }
    Ok(())
}

fn rebuild_projection_plan(activation: &mut Value) -> NorthResult<()> {
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

fn publish(mut activation: Value) -> NorthResult<Value> {
    object_mut(&mut activation)?.insert("schema".into(), Value::String(ACTIVATION_SCHEMA.into()));
    object_mut(&mut activation)?.remove("generationId");
    let generation_id = format!("sha256:{}", sha256(&serde_json::to_vec(&activation)?));
    object_mut(&mut activation)?
        .insert("generationId".into(), Value::String(generation_id.clone()));

    let root = agents_root()?;
    fs::create_dir_all(&root)?;
    let suffix = generation_id.trim_start_matches("sha256:");
    let generation = root.join(format!("gen-{suffix}"));
    if !generation.is_dir() {
        let temporary = root.join(format!(".gen-{suffix}.tmp-{}", std::process::id()));
        if temporary.exists() {
            fs::remove_dir_all(&temporary)?;
        }
        let current = current_generation_path()?;
        copy_tree(&current, &temporary)?;
        refresh_generation(&temporary, &activation)?;
        write_json(&temporary.join("activation.json"), &activation)?;
        fs::rename(&temporary, &generation)?;
    }
    atomic_symlink(&root.join("current"), Path::new(&format!("gen-{suffix}")))?;
    publish_codex_links(&activation)?;
    Ok(activation)
}

fn refresh_generation(generation: &Path, activation: &Value) -> NorthResult<()> {
    for unit in units(activation)? {
        if !unit.get("active").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let id = string_field(unit, "id")?;
        for distribution in unit
            .get("distributions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if string_field(distribution, "type")? == "skill"
                && distribution
                    .get("targets")
                    .and_then(Value::as_array)
                    .is_some_and(|targets| {
                        targets
                            .iter()
                            .any(|target| target.as_str() == Some("shared"))
                    })
            {
                let source = owner_path(owner(distribution)?)?;
                let target = generation.join("skills/shared").join(id);
                replace_copy(&source, &target)?;
            }
        }
    }
    refresh_instructions(generation, activation, "shared")?;
    refresh_instructions(generation, activation, "codex")?;
    Ok(())
}

fn refresh_instructions(generation: &Path, activation: &Value, target: &str) -> NorthResult<()> {
    let mut content = String::new();
    for baseline in activation
        .get("baselines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if baseline
            .get("targets")
            .and_then(Value::as_array)
            .is_some_and(|targets| targets.iter().any(|item| item.as_str() == Some(target)))
        {
            let owner = owner(baseline)?;
            content.push_str(&format!(
                "<!-- {}:{} -->\n",
                string_field(owner, "repo")?,
                string_field(owner, "path")?
            ));
            content.push_str(&fs::read_to_string(owner_path(owner)?)?);
            content.push_str("\n\n");
        }
    }
    if !content.is_empty() {
        let path = generation
            .join("instructions")
            .join(target)
            .join("AGENTS.md");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
    }
    Ok(())
}

fn publish_codex_links(activation: &Value) -> NorthResult<()> {
    let home = home()?;
    let directory = home.join(".codex/skills");
    fs::create_dir_all(&directory)?;
    let generation = current_generation_path()?;
    let mut managed = Vec::new();
    for unit in units(activation)? {
        let id = string_field(unit, "id")?;
        let shared_skill = unit.get("active").and_then(Value::as_bool).unwrap_or(false)
            && unit
                .get("distributions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|distribution| {
                    distribution.get("type").and_then(Value::as_str) == Some("skill")
                        && distribution
                            .get("targets")
                            .and_then(Value::as_array)
                            .is_some_and(|targets| {
                                targets
                                    .iter()
                                    .any(|target| target.as_str() == Some("shared"))
                            })
                });
        let link = directory.join(id);
        if shared_skill {
            atomic_symlink(&link, &generation.join("skills/shared").join(id))?;
            managed.push(id.to_owned());
        } else if link.is_symlink() {
            let target = fs::read_link(&link)?;
            if target.starts_with(&generation)
                || target.to_string_lossy().contains("/north/agents/")
            {
                fs::remove_file(link)?;
            }
        }
    }
    managed.sort();
    write_json(
        &agents_root()?.join("codex-managed-skills.json"),
        &json!({"schema": MANAGED_SKILLS_SCHEMA, "ids": managed}),
    )?;
    Ok(())
}

fn with_lock<T>(operation: impl FnOnce() -> NorthResult<T>) -> NorthResult<T> {
    let root = agents_root()?;
    fs::create_dir_all(&root)?;
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .open(root.join(".lock"))?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|error| {
        NorthError::Configuration(format!("cannot acquire agent activation lock: {error}"))
    })?;
    operation()
}

fn current_generation_path() -> NorthResult<PathBuf> {
    let current = agents_root()?.join("current");
    fs::canonicalize(&current).map_err(|error| {
        NorthError::Configuration(format!(
            "cannot resolve current activation {}: {error}",
            current.display()
        ))
    })
}

fn read_current() -> NorthResult<Value> {
    read_json(&agents_root()?.join("current/activation.json"))
}

fn read_json(path: &Path) -> NorthResult<Value> {
    let bytes = fs::read(path).map_err(|error| {
        NorthError::Configuration(format!("cannot read {}: {error}", path.display()))
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_json(path: &Path, value: &Value) -> NorthResult<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> NorthResult<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
        let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if source.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    } else {
        return configuration(format!(
            "refusing unsupported source type: {}",
            source.display()
        ));
    }
    Ok(())
}

fn replace_copy(source: &Path, target: &Path) -> NorthResult<()> {
    if target.is_dir() {
        fs::remove_dir_all(target)?;
    } else if target.exists() || target.is_symlink() {
        fs::remove_file(target)?;
    }
    copy_tree(source, target)
}

fn atomic_symlink(link: &Path, target: &Path) -> NorthResult<()> {
    let parent = link
        .parent()
        .ok_or_else(|| NorthError::Configuration("link has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".north-link-{}.tmp", std::process::id()));
    if temporary.exists() || temporary.is_symlink() {
        fs::remove_file(&temporary)?;
    }
    symlink(target, &temporary)?;
    fs::rename(temporary, link)?;
    Ok(())
}

fn provenance(owner: &Value) -> NorthResult<Value> {
    let source = owner_path(owner)?;
    Ok(json!({
        "owner": owner,
        "revision": repo_revision(string_field(owner, "repo")?)?,
        "contentDigest": format!("sha256:{}", digest_source(&source)?),
    }))
}

fn digest_source(path: &Path) -> NorthResult<String> {
    let mut digest = Sha256::new();
    digest_path(path, path, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

fn digest_path(root: &Path, path: &Path, digest: &mut Sha256) -> NorthResult<()> {
    digest.update(
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .as_bytes(),
    );
    digest.update([0]);
    if path.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            digest_path(root, &entry.path(), digest)?;
        }
    } else if path.is_file() {
        digest.update(fs::read(path)?);
    } else {
        return configuration(format!(
            "owner source has unsupported type: {}",
            path.display()
        ));
    }
    Ok(())
}

fn catalog_digest() -> NorthResult<String> {
    let paths = [
        repo_root("north-v2")?.join("agent-machinery/catalog.json"),
        repo_root("nixos-config")?.join("dotfiles/agents/catalog-config.json"),
    ];
    let mut digest = Sha256::new();
    for path in paths {
        digest.update(fs::read(path)?);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn repo_revision(repo: &str) -> NorthResult<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_root(repo)?.display().to_string(),
            "rev-parse",
            "HEAD",
        ])
        .output()?;
    if !output.status.success() {
        return configuration(format!("cannot resolve revision for {repo}"));
    }
    String::from_utf8(output.stdout)
        .map(|revision| revision.trim().to_owned())
        .map_err(|error| NorthError::Configuration(format!("invalid revision for {repo}: {error}")))
}

fn repo_root(repo: &str) -> NorthResult<PathBuf> {
    if let Ok(configured) = env::var("NORTH_REPO_ROOTS") {
        let roots: Value = serde_json::from_str(&configured)?;
        if let Some(path) = roots.get(repo).and_then(Value::as_str) {
            return Ok(PathBuf::from(path));
        }
    }
    if repo == "north" {
        if let Ok(path) = env::var("NORTH_HOME") {
            return Ok(PathBuf::from(path));
        }
    }
    Ok(home()?.join("code").join(repo).join("main"))
}

fn owner_path(owner: &Value) -> NorthResult<PathBuf> {
    let root = repo_root(string_field(owner, "repo")?)?;
    let relative = Path::new(string_field(owner, "path")?);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| component.as_os_str() == "..")
    {
        return configuration("owner path escapes its repository");
    }
    let path = root.join(relative);
    if !path.exists() {
        return configuration(format!("owner source does not exist: {}", path.display()));
    }
    Ok(path)
}

fn agents_root() -> NorthResult<PathBuf> {
    Ok(env::var_os("NORTH_AGENT_STATE_ROOT")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".local/state/north/agents")))
}

fn home() -> NorthResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| NorthError::Configuration("HOME is unavailable".into()))
}

fn skill_metadata(path: &Path) -> NorthResult<BTreeMap<String, String>> {
    let source = fs::read_to_string(path)?;
    let mut result = BTreeMap::new();
    let mut lines = source.lines();
    if lines.next() != Some("---") {
        return configuration(format!("skill has invalid frontmatter: {}", path.display()));
    }
    let frontmatter = lines.take_while(|line| *line != "---").collect::<Vec<_>>();
    let mut index = 0;
    while index < frontmatter.len() {
        if let Some((key, raw)) = frontmatter[index].split_once(':') {
            let raw = raw.trim();
            if [">", ">-", "|", "|-"].contains(&raw) {
                index += 1;
                let mut parts = Vec::new();
                while index < frontmatter.len()
                    && (frontmatter[index].starts_with(' ') || frontmatter[index].trim().is_empty())
                {
                    let text = frontmatter[index].trim();
                    if !text.is_empty() {
                        parts.push(text);
                    }
                    index += 1;
                }
                result.insert(
                    key.into(),
                    parts.join(if raw.starts_with('>') { " " } else { "\n" }),
                );
                continue;
            }
            result.insert(key.into(), raw.trim_matches(['\'', '"']).into());
        }
        index += 1;
    }
    Ok(result)
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

fn find_unit_mut<'a>(activation: &'a mut Value, id: &str) -> NorthResult<&'a mut Value> {
    units_mut(activation)?
        .iter_mut()
        .find(|unit| unit.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| NorthError::Configuration(format!("unknown unit: {id}")))
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
    "usage: north config agents [sync|status|on|off|path|inspect] ...".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_package_registration_resolves_source_beside_catalog() {
        let registration = catalog_registration(
            &json!({
                "id": "example-distilled",
                "kind": "skill",
                "source": "skills/example-distilled/SKILL.md"
            }),
            &json!({
                "repo": "north-v2",
                "path": "agent-machinery/catalog.json"
            }),
        )
        .expect("project package registration must resolve");
        assert_eq!(
            registration.get("owner"),
            Some(&json!({
                "repo": "north-v2",
                "path": "agent-machinery/skills/example-distilled/SKILL.md"
            }))
        );
    }

    #[test]
    fn parses_clause_skill_frontmatter() {
        let metadata = skill_metadata(Path::new(
            "/home/tom/code/nixos-config/main/dotfiles/agents/skills/clause-authoring-distilled/SKILL.md",
        ))
        .expect("frontmatter must parse");
        assert_eq!(
            metadata.get("name").map(String::as_str),
            Some("clause-authoring-distilled")
        );
        assert!(
            metadata
                .get("description")
                .is_some_and(|description| description.contains(".clause"))
        );
    }
}
