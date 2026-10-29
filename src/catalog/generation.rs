use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};

use super::files::{atomic_symlink, copy_tree, current_generation_path, replace_copy, write_json};
use super::paths::{agents_root, home, owner_path};
use super::{
    ACTIVATION_SCHEMA, MANAGED_SKILLS_SCHEMA, configuration, object_mut, owner, sha256, string_field, units,
};

pub(super) fn publish(mut activation: Value) -> NorthResult<Value> {
    object_mut(&mut activation)?.insert("schema".into(), Value::String(ACTIVATION_SCHEMA.into()));
    object_mut(&mut activation)?.remove("generationId");
    let generation_id = format!("sha256:{}", sha256(&serde_json::to_vec(&activation)?));
    object_mut(&mut activation)?
        .insert("generationId".into(), Value::String(generation_id.clone()));

    publish_generation(&activation, &agents_root()?)?;
    publish_skill_links(&activation)?;
    Ok(activation)
}

pub(super) fn publish_generation(activation: &Value, root: &Path) -> NorthResult<()> {
    fs::create_dir_all(root)?;
    let generation_id = string_field(activation, "generationId")?;
    let suffix = generation_id.trim_start_matches("sha256:");
    let generation = root.join(format!("gen-{suffix}"));
    if !generation.is_dir() {
        let temporary = root.join(format!(".gen-{suffix}.tmp-{}", std::process::id()));
        start_clean_generation(&temporary)?;
        refresh_generation(&temporary, activation)?;
        write_json(&temporary.join("activation.json"), activation)?;
        fs::rename(&temporary, &generation)?;
    }
    atomic_symlink(&root.join("current"), Path::new(&format!("gen-{suffix}")))?;
    Ok(())
}

pub(super) fn start_clean_generation(generation: &Path) -> NorthResult<()> {
    if generation.is_dir() {
        fs::remove_dir_all(generation)?;
    } else if generation.exists() || generation.is_symlink() {
        fs::remove_file(generation)?;
    }
    fs::create_dir_all(generation)?;
    Ok(())
}

fn refresh_generation(generation: &Path, activation: &Value) -> NorthResult<()> {
    refresh_provider_hooks(generation, activation)?;
    refresh_shared_skills(generation, activation)?;
    write_active_list(generation, activation)?;
    let instructions = generation.join("instructions");
    if instructions.is_dir() {
        fs::remove_dir_all(&instructions)?;
    }
    for target in instruction_targets(activation)? {
        refresh_instructions(generation, activation, &target)?;
    }
    Ok(())
}

pub(super) fn write_active_list(generation: &Path, activation: &Value) -> NorthResult<()> {
    let mut list = String::new();
    for unit in units(activation)? {
        if unit.get("active").and_then(Value::as_bool) == Some(true) {
            list.push_str(&format!(
                "{} {}\n",
                string_field(unit, "kind")?,
                string_field(unit, "id")?
            ));
        }
    }
    fs::write(generation.join("activation.active"), list)?;
    Ok(())
}

fn refresh_shared_skills(generation: &Path, activation: &Value) -> NorthResult<()> {
    let directory = generation.join("skills");
    if directory.is_dir() {
        fs::remove_dir_all(&directory)?;
    }
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
            if string_field(distribution, "type")? != "skill" {
                continue;
            }
            let source = owner_path(owner(distribution)?)?;
            for target in distribution
                .get("targets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let target = target
                    .as_str()
                    .ok_or_else(|| NorthError::Configuration("invalid skill target".into()))?;
                replace_copy(
                    &source,
                    &directory.join(safe_relative_path(target)?).join(id),
                )?;
            }
        }
    }
    Ok(())
}

fn refresh_provider_hooks(generation: &Path, activation: &Value) -> NorthResult<()> {
    let directory = generation.join("provider-hooks");
    if directory.is_dir() {
        fs::remove_dir_all(&directory)?;
    }
    fs::create_dir_all(&directory)?;
    let mut destinations = BTreeMap::<PathBuf, Value>::new();

    for support in activation
        .get("providerSupport")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            NorthError::Configuration("activation providerSupport must be an array".into())
        })?
    {
        let relative = safe_relative_path(string_field(support, "path")?)?;
        add_provider_hook_destination(&mut destinations, relative, owner(support)?.clone())?;
    }

    for unit in units(activation)? {
        let id = string_field(unit, "id")?;
        for distribution in unit
            .get("distributions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let distribution_type = string_field(distribution, "type")?;
            if distribution_type != "hook" && distribution_type != "providerAdapter" {
                continue;
            }
            let distribution_owner = owner(distribution)?.clone();
            let source = owner_path(&distribution_owner)?;
            let relative = if distribution_type == "providerAdapter" {
                safe_relative_path(string_field(distribution, "adapterId")?)?
            } else {
                PathBuf::from(source.file_name().ok_or_else(|| {
                    NorthError::Configuration(format!("hook {id} source has no file name"))
                })?)
            };
            add_provider_hook_destination(&mut destinations, relative, distribution_owner)?;
        }
    }

    for (relative, source_owner) in destinations {
        copy_tree(&owner_path(&source_owner)?, &directory.join(relative))?;
    }
    super::retired_hooks::write_stubs(&directory)?;
    Ok(())
}

fn add_provider_hook_destination(
    destinations: &mut BTreeMap<PathBuf, Value>,
    relative: PathBuf,
    source_owner: Value,
) -> NorthResult<()> {
    add_projection_destination(destinations, relative, source_owner, "provider hook")
}

fn add_projection_destination(
    destinations: &mut BTreeMap<PathBuf, Value>,
    relative: PathBuf,
    source_owner: Value,
    kind: &str,
) -> NorthResult<()> {
    if let Some(existing) = destinations.get(&relative) {
        if existing != &source_owner {
            return configuration(format!(
                "{kind} destination {} has multiple owners",
                relative.display()
            ));
        }
        return Ok(());
    }
    destinations.insert(relative, source_owner);
    Ok(())
}

pub(super) fn safe_relative_path(value: &str) -> NorthResult<PathBuf> {
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| component.as_os_str() == "..")
    {
        return configuration(format!("projection path escapes its generation: {value}"));
    }
    Ok(path)
}

pub(super) fn instruction_targets(activation: &Value) -> NorthResult<BTreeSet<String>> {
    let mut targets = BTreeSet::new();
    for baseline in activation
        .get("baselines")
        .and_then(Value::as_array)
        .ok_or_else(|| NorthError::Configuration("activation baselines must be an array".into()))?
    {
        collect_targets(baseline, &mut targets)?;
    }
    for unit in units(activation)? {
        if !unit.get("active").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        for distribution in unit
            .get("distributions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if string_field(distribution, "type")? == "instructions" {
                collect_targets(distribution, &mut targets)?;
            }
        }
    }
    Ok(targets)
}

fn collect_targets(value: &Value, targets: &mut BTreeSet<String>) -> NorthResult<()> {
    for target in value
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| NorthError::Configuration("projection targets must be an array".into()))?
    {
        targets.insert(
            target
                .as_str()
                .ok_or_else(|| {
                    NorthError::Configuration("projection target must be a string".into())
                })?
                .to_owned(),
        );
    }
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
    for unit in units(activation)? {
        if !unit.get("active").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        for distribution in unit
            .get("distributions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if string_field(distribution, "type")? != "instructions"
                || !distribution
                    .get("targets")
                    .and_then(Value::as_array)
                    .is_some_and(|targets| targets.iter().any(|item| item.as_str() == Some(target)))
            {
                continue;
            }
            let distribution_owner = owner(distribution)?;
            content.push_str(&format!(
                "<!-- {}:{} -->\n",
                string_field(distribution_owner, "repo")?,
                string_field(distribution_owner, "path")?
            ));
            content.push_str(&fs::read_to_string(owner_path(distribution_owner)?)?);
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

// Codex and Claude Code each discover skills only as direct children of their
// own skills directory, so each receives links for its allowed skills.
fn publish_skill_links(activation: &Value) -> NorthResult<()> {
    let root = agents_root()?;
    let generation = current_generation_path()?;
    for (directory, manifest, agent) in [
        (".codex/skills", "codex-managed-skills.json", "codex"),
        (".claude/skills", "claude-managed-skills.json", "claude"),
    ] {
        publish_skill_links_in(
            &home()?.join(directory),
            manifest,
            &root,
            &generation,
            activation,
            agent,
        )?;
    }
    Ok(())
}

pub(super) fn publish_skill_links_in(
    directory: &Path,
    manifest: &str,
    root: &Path,
    generation: &Path,
    activation: &Value,
    agent: &str,
) -> NorthResult<()> {
    fs::create_dir_all(directory)?;
    let manifest = root.join(manifest);
    let previous = fs::read(&manifest)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.get("ids").and_then(Value::as_array).cloned())
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut managed = BTreeSet::new();
    for unit in units(activation)? {
        let id = string_field(unit, "id")?;
        let target = unit
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            .then(|| {
                unit.get("distributions")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|distribution| {
                        distribution.get("type").and_then(Value::as_str) == Some("skill")
                    })
                    .flat_map(|distribution| {
                        distribution
                            .get("targets")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                    })
                    .filter_map(Value::as_str)
                    .find(|target| *target == "shared" || *target == agent)
            })
            .flatten();
        if let Some(target) = target {
            atomic_symlink(
                &directory.join(id),
                &generation.join("skills").join(target).join(id),
            )?;
            managed.insert(id.to_owned());
        }
    }

    // A link North published earlier is North's to retire once its id leaves the
    // generation, whether the manifest or only the link target records that origin.
    let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if managed.contains(&name) || !entry.file_type()?.is_symlink() {
            continue;
        }
        let target = fs::read_link(entry.path())?;
        if previous.contains(&name)
            || target.starts_with(root)
            || target.starts_with(&canonical_root)
        {
            fs::remove_file(entry.path())?;
        }
    }
    write_json(
        &manifest,
        &json!({"schema": MANAGED_SKILLS_SCHEMA, "ids": managed}),
    )?;
    Ok(())
}
