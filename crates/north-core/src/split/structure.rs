use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use toml_edit::DocumentMut;

use super::paths::glob;
use super::{Module, Plan};

pub struct Modules {
    pub paths: BTreeMap<String, Vec<String>>,
    pub may_use: BTreeMap<String, Vec<String>>,
    pub facades: BTreeSet<String>,
}

pub fn parse(text: &str) -> Result<Modules, String> {
    let document = text.parse::<DocumentMut>().map_err(|error| format!("structure.toml: {error}"))?;
    let mut modules = Modules { paths: BTreeMap::new(), may_use: BTreeMap::new(), facades: BTreeSet::new() };
    let Some(tables) = document.get("module").and_then(|item| item.as_array_of_tables()) else { return Ok(modules) };
    let strings = |item: Option<&toml_edit::Item>| item.and_then(|item| item.as_array()).map(|array| array.iter().filter_map(|value| value.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
    for table in tables {
        let name = table.get("name").and_then(|item| item.as_str()).ok_or("structure.toml: a [[module]] has no name")?.to_string();
        modules.paths.insert(name.clone(), strings(table.get("paths")));
        if table.get("facade").and_then(|item| item.as_bool()).unwrap_or(false) {
            modules.facades.insert(name.clone());
        }
        modules.may_use.insert(name, strings(table.get("may_use")));
    }
    Ok(modules)
}

pub fn cycle(modules: &Modules) -> Option<Vec<String>> {
    let mut done = BTreeSet::new();
    for start in modules.may_use.keys() {
        let mut stack = Vec::new();
        if let Some(found) = visit(modules, start, &mut stack, &mut done) {
            return Some(found);
        }
    }
    None
}

fn visit(modules: &Modules, node: &str, stack: &mut Vec<String>, done: &mut BTreeSet<String>) -> Option<Vec<String>> {
    if let Some(at) = stack.iter().position(|seen| seen == node) {
        let mut found = stack[at..].to_vec();
        found.push(node.to_string());
        return Some(found);
    }
    if done.contains(node) {
        return None;
    }
    stack.push(node.to_string());
    let uses = if modules.facades.contains(node) { None } else { modules.may_use.get(node) };
    for next in uses.into_iter().flatten().filter(|next| *next != node) {
        if let Some(found) = visit(modules, next, stack, done) {
            return Some(found);
        }
    }
    stack.pop();
    done.insert(node.to_string());
    None
}

fn read(plan: &Plan) -> Option<String> {
    fs::read_to_string(plan.root.join("structure.toml")).ok()
}

fn owner(modules: &Modules, file: &str) -> Option<String> {
    modules.paths.iter().find(|(name, paths)| *name != "tests" && paths.iter().any(|pattern| glob(pattern, file))).map(|(name, _)| name.clone())
}

pub fn owners(plan: &Plan, pending: &[Module]) -> Result<Vec<Option<String>>, String> {
    let Some(text) = read(plan) else { return Ok(vec![None; pending.len()]) };
    let modules = parse(&text)?;
    Ok(pending.iter().map(|module| owner(&modules, &probe(&format!("crates/{}/src/{}", plan.from, module.path), module))).collect())
}

fn probe(base: &str, module: &Module) -> String {
    if module.old_dir.is_some() { format!("{base}/mod.rs") } else { format!("{base}.rs") }
}

pub fn cover(plan: &Plan, pending: &[Module], owners: &[Option<String>]) -> Result<(), String> {
    let Some(mut text) = read(plan) else { return Ok(()) };
    let before = parse(&text)?;
    let lib = format!("crates/{}/src/lib.rs", plan.to);
    for (module, owner) in pending.iter().zip(owners) {
        let Some(owner) = owner else { continue };
        let base = format!("crates/{}/src/{}", plan.to, module.name);
        let modules = parse(&text)?;
        if !modules.paths[owner].iter().any(|pattern| glob(pattern, &probe(&base, module))) {
            let pattern = if module.old_dir.is_some() { format!("{base}/**") } else { format!("{base}.rs") };
            text = add_path(&text, owner, &pattern, false)?;
        }
    }
    let explicit = parse(&text)?.paths.values().flatten().any(|pattern| *pattern == lib);
    if let (false, Some(Some(first))) = (explicit, owners.first()) {
        text = add_path(&text, first, &lib, true)?;
    }
    let after = parse(&text)?;
    if before.may_use != after.may_use {
        return Err("structure.toml: a may_use list changed during the move".into());
    }
    if let Some(found) = cycle(&after) {
        return Err(format!("structure.toml: may_use cycle {}", found.join(" -> ")));
    }
    fs::write(plan.root.join("structure.toml"), text).map_err(|error| error.to_string())
}

fn add_path(text: &str, module: &str, pattern: &str, front: bool) -> Result<String, String> {
    let mut out = String::new();
    let mut inside = false;
    let mut added = false;
    for line in text.lines() {
        inside |= line == format!("name = \"{module}\"");
        let mut line = line.to_string();
        if inside && !added && line.starts_with("paths = [") && line.ends_with(']') {
            let quoted = format!("\"{pattern}\"");
            line = if front { line.replacen("paths = [", &format!("paths = [{quoted}, "), 1) } else { format!("{}, {quoted}]", line.trim_end_matches(']')) };
            added = true;
            inside = false;
        }
        out.push_str(&line);
        out.push('\n');
    }
    if !added {
        return Err(format!("structure.toml: module {module} has no one-line paths list"));
    }
    Ok(out)
}
