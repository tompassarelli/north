use super::snapshot::Snapshot;
use crate::code_health::rust_text::non_test_lines;
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item};

const KEYS: [&str; 4] = ["default", "hub", "hub_dependents", "overrides"];

pub(super) struct Caps {
    default: usize,
    hub: usize,
    hub_dependents: usize,
    overrides: BTreeMap<String, usize>,
    pub(super) enrolled: bool,
}

impl Caps {
    pub(super) fn read(snapshot: &Snapshot) -> Result<Self, String> {
        let mut caps = Self { default: 25_000, hub: 10_000, hub_dependents: 3, overrides: BTreeMap::new(), enrolled: false };
        let Some(text) = snapshot.text("structure.toml") else { return Ok(caps) };
        let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| format!("structure.toml: {error}"))?;
        let Some(table) = document.get("crate_lines") else { return Ok(caps) };
        let table = table.as_table_like().ok_or("structure.toml: crate_lines must be a table")?;
        caps.enrolled = true;
        for (key, item) in table.iter() {
            match key {
                "default" => caps.default = number(item, key)?,
                "hub" => caps.hub = number(item, key)?,
                "hub_dependents" => caps.hub_dependents = number(item, key)?,
                "overrides" => caps.overrides = overrides(item)?,
                _ => return Err(format!("structure.toml: unknown key crate_lines.{key}; known: {}", KEYS.join(", "))),
            }
        }
        Ok(caps)
    }

    fn of(&self, name: &str, dependents: usize) -> usize {
        self.overrides.get(name).copied().unwrap_or(if dependents >= self.hub_dependents { self.hub } else { self.default })
    }
}

fn number(item: &Item, key: &str) -> Result<usize, String> {
    item.as_integer().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| format!("structure.toml: crate_lines.{key} must be a non-negative integer"))
}

fn overrides(item: &Item) -> Result<BTreeMap<String, usize>, String> {
    let table = item.as_table_like().ok_or("structure.toml: crate_lines.overrides must be a table of crate = lines")?;
    table.iter().map(|(name, cap)| number(cap, &format!("overrides.{name}")).map(|cap| (name.to_owned(), cap))).collect()
}

pub(super) struct Measured {
    pub(super) name: String,
    pub(super) dir: String,
    pub(super) lines: usize,
    pub(super) cap: usize,
    pub(super) dependents: usize,
    pub(super) depends_on: Vec<String>,
    pub(super) files: BTreeMap<String, usize>,
}

struct Member {
    name: String,
    dir: String,
    depends_on: Vec<String>,
}

pub(super) fn census(snapshot: &Snapshot, caps: &Caps) -> Vec<Measured> {
    let members = members(snapshot);
    let mut measured: Vec<Measured> = members
        .iter()
        .map(|member| {
            let dependents = members.iter().filter(|other| other.depends_on.contains(&member.name)).count();
            Measured { name: member.name.clone(), dir: member.dir.clone(), lines: 0, cap: caps.of(&member.name, dependents), dependents, depends_on: member.depends_on.clone(), files: BTreeMap::new() }
        })
        .collect();
    for (path, text) in snapshot.files().filter(|(path, _)| path.ends_with(".rs")) {
        let owner = measured.iter_mut().filter(|krate| owns(&krate.dir, path)).max_by_key(|krate| krate.dir.len());
        if let Some(krate) = owner {
            let lines = non_test_lines(text);
            krate.lines += lines;
            krate.files.insert(path.to_owned(), lines);
        }
    }
    measured
}

fn owns(dir: &str, path: &str) -> bool {
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    path.strip_prefix(&prefix).is_some_and(|rest| rest.starts_with("src/") || rest == "build.rs")
}

fn members(snapshot: &Snapshot) -> Vec<Member> {
    let Some(root) = snapshot.text("Cargo.toml").and_then(|text| text.parse::<DocumentMut>().ok()) else { return Vec::new() };
    let mut dirs: Vec<String> = root.get("package").map(|_| String::new()).into_iter().collect();
    let listed = root.get("workspace").and_then(|workspace| workspace.get("members")).and_then(Item::as_array);
    for pattern in listed.into_iter().flatten().filter_map(|value| value.as_str()) {
        dirs.extend(expand(snapshot, pattern.trim_end_matches('/')));
    }
    dirs.sort();
    dirs.dedup();
    let manifests: Vec<(String, DocumentMut)> = dirs.into_iter().filter_map(|dir| manifest(snapshot, &dir).map(|document| (dir, document))).collect();
    let names: Vec<String> = manifests.iter().filter_map(|(_, document)| package_name(document)).collect();
    manifests
        .iter()
        .filter_map(|(dir, document)| {
            let depends_on = dependency_names(document).into_iter().filter(|name| names.contains(name)).collect();
            Some(Member { name: package_name(document)?, dir: dir.clone(), depends_on })
        })
        .collect()
}

fn expand(snapshot: &Snapshot, pattern: &str) -> Vec<String> {
    let Some(parent) = pattern.strip_suffix("/*") else { return vec![pattern.to_owned()] };
    snapshot
        .files()
        .filter_map(|(path, _)| path.strip_suffix("/Cargo.toml")?.strip_prefix(&format!("{parent}/")).filter(|child| !child.contains('/')).map(|child| format!("{parent}/{child}")))
        .collect()
}

fn manifest(snapshot: &Snapshot, dir: &str) -> Option<DocumentMut> {
    let path = if dir.is_empty() { "Cargo.toml".to_owned() } else { format!("{dir}/Cargo.toml") };
    snapshot.text(&path)?.parse().ok()
}

fn package_name(document: &DocumentMut) -> Option<String> {
    document.get("package")?.get("name")?.as_str().map(str::to_owned)
}

fn dependency_names(document: &DocumentMut) -> Vec<String> {
    let Some(table) = document.get("dependencies").and_then(Item::as_table_like) else { return Vec::new() };
    table.iter().map(|(key, item)| item.get("package").and_then(Item::as_str).unwrap_or(key).to_owned()).collect()
}
