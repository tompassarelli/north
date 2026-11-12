use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use toml_edit::{DocumentMut, Item, Value};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Member {
    Package(String),
    Directory(String),
}

pub fn input(path: &str) -> bool {
    path == "Cargo.lock" || path == "Cargo.toml"
}

pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) source: bool,
    entry: String,
    pub(crate) dependencies: Vec<String>,
}

pub(crate) fn packages(text: &str) -> Result<Vec<Package>, String> {
    let document: DocumentMut = text.parse().map_err(|why| format!("Cargo.lock does not parse: {why}"))?;
    let Some(tables) = document.get("package").and_then(Item::as_array_of_tables) else {
        return Err("Cargo.lock lists no [[package]]".to_owned());
    };
    tables
        .iter()
        .map(|table| {
            let field = |key: &str| table.get(key).and_then(Item::as_str).map(str::to_owned);
            let name = field("name").ok_or("a Cargo.lock [[package]] has no name")?;
            let listed: Vec<String> = table.get("dependencies").and_then(Item::as_array).map(|array| array.iter().filter_map(|value| value.as_str()).map(str::to_owned).collect()).unwrap_or_default();
            let dependencies: Vec<String> = listed.iter().filter_map(|text| text.split_whitespace().next()).map(str::to_owned).collect();
            let entry = format!("{name} {:?} {:?} {:?} {:?}", field("version"), field("source"), field("checksum"), listed);
            Ok(Package { name, source: field("source").is_some(), entry, dependencies })
        })
        .collect()
}

pub fn lock_members(base: &str, head: &str) -> Result<BTreeSet<Member>, String> {
    let before: BTreeSet<String> = packages(base)?.into_iter().map(|package| package.entry).collect();
    let after = packages(head)?;
    let changed: Vec<&Package> = after.iter().filter(|package| !before.contains(&package.entry)).collect();
    let mut dependents: BTreeMap<&str, Vec<&Package>> = BTreeMap::new();
    for package in &after {
        for dependency in &package.dependencies {
            dependents.entry(dependency.as_str()).or_default().push(package);
        }
    }
    let mut members = BTreeSet::new();
    let mut seen: BTreeSet<&str> = changed.iter().map(|package| package.name.as_str()).collect();
    let mut queue: Vec<&Package> = changed.clone();
    while let Some(package) = queue.pop() {
        if !package.source {
            members.insert(Member::Package(package.name.clone()));
        }
        for dependent in dependents.get(package.name.as_str()).into_iter().flatten() {
            if seen.insert(dependent.name.as_str()) {
                queue.push(dependent);
            }
        }
    }
    Ok(members)
}

fn plain_value(value: &Value) -> serde_json::Value {
    match value {
        Value::String(text) => json!(text.value()),
        Value::Integer(number) => json!(number.value()),
        Value::Float(number) => json!(number.value()),
        Value::Boolean(flag) => json!(flag.value()),
        Value::Datetime(moment) => json!(format!("{:?}", moment.value())),
        Value::Array(array) => serde_json::Value::Array(array.iter().map(plain_value).collect()),
        Value::InlineTable(table) => serde_json::Value::Object(table.iter().map(|(key, value)| (key.to_owned(), plain_value(value))).collect()),
    }
}

fn plain_table<'a>(entries: impl Iterator<Item = (&'a str, &'a Item)>) -> serde_json::Value {
    serde_json::Value::Object(entries.map(|(key, item)| (key.to_owned(), plain(item))).collect())
}

fn plain(item: &Item) -> serde_json::Value {
    match item {
        Item::None => serde_json::Value::Null,
        Item::Value(value) => plain_value(value),
        Item::Table(table) => plain_table(table.iter()),
        Item::ArrayOfTables(tables) => serde_json::Value::Array(tables.iter().map(|table| plain_table(table.iter())).collect()),
    }
}

type Json = serde_json::Value;

static NULL: Json = Json::Null;

const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

fn manifest(text: &str) -> Result<Json, String> {
    let document: DocumentMut = text.parse().map_err(|why| format!("a Cargo.toml does not parse: {why}"))?;
    Ok(plain(document.as_item()))
}

fn keys<'a>(before: &'a Json, after: &'a Json) -> BTreeSet<&'a str> {
    [before, after].into_iter().filter_map(Json::as_object).flat_map(|table| table.keys().map(String::as_str)).collect()
}

fn names(value: &Json) -> BTreeSet<String> {
    value.as_array().into_iter().flatten().filter_map(Json::as_str).map(str::to_owned).collect()
}

fn package_name(manifest: &Json) -> Option<String> {
    manifest.pointer("/package/name").and_then(Json::as_str).map(str::to_owned)
}

fn inherits(manifest: &Json, dependency: &str) -> bool {
    DEPENDENCY_TABLES.iter().any(|table| manifest.get(table).and_then(|deps| deps.get(dependency)).and_then(|entry| entry.get("workspace")) == Some(&Json::Bool(true)))
}

fn workspace_change<'a>(old: &'a Json, new: &'a Json, members: &mut BTreeSet<Member>, inherited: &mut BTreeSet<&'a str>) -> Result<(), String> {
    for part in keys(old, new) {
        let (old, new) = (old.get(part).unwrap_or(&NULL), new.get(part).unwrap_or(&NULL));
        match part {
            _ if old == new => {}
            "members" => members.extend(names(old).symmetric_difference(&names(new)).filter(|member| *member != ".").map(|member| Member::Directory(member.clone()))),
            "default-members" => {}
            "dependencies" => inherited.extend(keys(old, new).into_iter().filter(|name| old.get(name) != new.get(name))),
            _ => return Err(format!("the root Cargo.toml changes [workspace.{part}]")),
        }
    }
    Ok(())
}

pub fn manifest_members(base: &str, head: &str, show_head: impl Fn(&str) -> Result<String, String>) -> Result<BTreeSet<Member>, String> {
    let (before, after) = (manifest(base)?, manifest(head)?);
    let mut members = BTreeSet::new();
    let mut inherited: BTreeSet<&str> = BTreeSet::new();
    for key in keys(&before, &after) {
        let (old, new) = (before.get(key).unwrap_or(&NULL), after.get(key).unwrap_or(&NULL));
        if old == new {
            continue;
        }
        match key {
            _ if DEPENDENCY_TABLES.contains(&key) => {
                members.insert(Member::Package(package_name(&after).ok_or("the root Cargo.toml changes its dependencies and names no package")?));
            }
            "workspace" => workspace_change(old, new, &mut members, &mut inherited)?,
            _ => return Err(format!("the root Cargo.toml changes its [{key}] table")),
        }
    }
    if inherited.is_empty() {
        return Ok(members);
    }
    let manifests = names(after.pointer("/workspace/members").unwrap_or(&NULL)).iter().map(|directory| show_head(&format!("{directory}/Cargo.toml")).and_then(|text| manifest(&text))).collect::<Result<Vec<_>, _>>()?;
    for member in std::iter::once(&after).chain(&manifests).filter(|member| inherited.iter().any(|dependency| inherits(member, dependency))) {
        members.extend(package_name(member).map(Member::Package));
    }
    Ok(members)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    pub members: BTreeSet<Member>,
    pub untraced: Option<String>,
}

impl Workspace {
    pub fn read(changed: &[&str], show: impl Fn(&str, &str) -> Result<String, String>, base: &str, head: &str) -> Self {
        let mut workspace = Self::default();
        for path in changed.iter().copied().filter(|path| input(path)) {
            let members = show(base, path).and_then(|before| Ok((before, show(head, path)?))).and_then(|(before, after)| if path == "Cargo.lock" { lock_members(&before, &after) } else { manifest_members(&before, &after, |member| show(head, member)) });
            match members {
                Ok(members) => workspace.members.extend(members),
                Err(why) => workspace.untraced = Some(why),
            }
        }
        workspace
    }
}
