//! The dependencies a lane adds, read from the manifests and lock file it changes: an entry of a dependency or patch table of a
//! Cargo.toml that its base lacks or holds with another value, and a Cargo.lock package its base lacks (north#1650).
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use crate::git;

const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
const LOCK: &str = "Cargo.lock";

fn source_at(directory: &Path, revision: &str, file: &str) -> String {
    git::text(directory, &["show", &format!("{revision}:{file}")]).unwrap_or_default()
}

fn value_shown(value: &Value) -> String {
    match value {
        Value::String(text) => format!("{:?}", text.value()),
        Value::Integer(number) => number.value().to_string(),
        Value::Float(number) => number.value().to_string(),
        Value::Boolean(flag) => flag.value().to_string(),
        Value::Datetime(time) => time.value().to_string(),
        Value::Array(items) => format!("[{}]", items.iter().map(value_shown).collect::<Vec<_>>().join(", ")),
        Value::InlineTable(table) => table_shown(table),
    }
}

fn table_shown(table: &dyn TableLike) -> String {
    format!("{{{}}}", table.iter().map(|(key, item)| format!("{key} = {}", shown(item))).collect::<Vec<_>>().join(", "))
}

pub(crate) fn shown(item: &Item) -> String {
    match item {
        Item::Value(value) => value_shown(value),
        Item::Table(table) => table_shown(table),
        Item::ArrayOfTables(tables) => format!("[{}]", tables.iter().map(|table| table_shown(table)).collect::<Vec<_>>().join(", ")),
        Item::None => String::new(),
    }
}

fn entries(table: &dyn TableLike, prefix: &str, into: &mut BTreeMap<String, String>) {
    for (name, item) in table.iter() {
        into.insert(format!("{prefix}{name}"), shown(item));
    }
}

fn dependency_tables(table: &Table, prefix: &str, into: &mut BTreeMap<String, String>) {
    for name in DEPENDENCY_TABLES {
        if let Some(found) = table.get(name).and_then(Item::as_table_like) {
            entries(found, &format!("{prefix}{name}."), into);
        }
    }
}

/// The dependency lines of a manifest, keyed by table and name: the dependency tables at its root, under `[workspace]` and under each
/// `[target.*]`, and each `[patch.*]` and `[replace]` table, which swap a dependency's source.
fn declared(manifest: &DocumentMut) -> BTreeMap<String, String> {
    let mut lines = BTreeMap::new();
    let root = manifest.as_table();
    dependency_tables(root, "", &mut lines);
    if let Some(workspace) = root.get("workspace").and_then(Item::as_table) {
        dependency_tables(workspace, "workspace.", &mut lines);
    }
    for (scope, item) in [("target", root.get("target")), ("patch", root.get("patch"))].into_iter().filter_map(|(scope, item)| Some((scope, item?.as_table()?))) {
        for (name, nested) in item.iter().filter_map(|(name, nested)| Some((name, nested.as_table()?))) {
            match scope {
                "target" => dependency_tables(nested, &format!("target.{name}."), &mut lines),
                _ => entries(nested, &format!("patch.{name}."), &mut lines),
            }
        }
    }
    if let Some(replace) = root.get("replace").and_then(Item::as_table) {
        entries(replace, "replace.", &mut lines);
    }
    lines
}

fn packages(lock: &DocumentMut) -> BTreeSet<String> {
    let field = |package: &Table, key: &str| package.get(key).and_then(Item::as_str).unwrap_or_default().to_owned();
    lock.get("package").and_then(Item::as_array_of_tables).map(|all| all.iter().map(|package| format!("{} {} {}", field(package, "name"), field(package, "version"), field(package, "source")).trim().to_owned()).collect()).unwrap_or_default()
}

/// What the lane `fork..head` adds among `files`, the files it changes: each manifest line `FILE adds TABLE.NAME`, each lock package
/// `Cargo.lock adds NAME VERSION SOURCE`, and each such file it leaves unparsable, which no reader can tell adds nothing.
pub(crate) fn added(directory: &Path, fork: &str, head: &str, files: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for file in files {
        let manifest = file == "Cargo.toml" || file.ends_with("/Cargo.toml");
        if !manifest && file != LOCK {
            continue;
        }
        let parse = |revision: &str| source_at(directory, revision, file).parse::<DocumentMut>();
        let Ok(before) = parse(fork).or_else(|_| "".parse()) else { continue };
        let Ok(after) = parse(head) else {
            found.push(format!("{file} does not parse"));
            continue;
        };
        if manifest {
            let base = declared(&before);
            found.extend(declared(&after).into_iter().filter(|(key, value)| base.get(key) != Some(value)).map(|(key, _)| format!("{file} adds {key}")));
        } else {
            let base = packages(&before);
            found.extend(packages(&after).into_iter().filter(|package| !base.contains(package)).map(|package| format!("{file} adds {package}")));
        }
    }
    found
}

/// Each file of `files`, the dependency-source files the lane `fork..head` changes, to which it adds a line, as `FILE adds lines`:
/// a pure deletion adds none, and a binary change, whose lines git cannot count, counts as an addition.
pub(crate) fn added_lines(directory: &Path, fork: &str, head: &str, files: &[&str]) -> Vec<String> {
    if files.is_empty() {
        return Vec::new();
    }
    let arguments: Vec<&str> = ["diff", "--numstat", "--no-renames", fork, head, "--"].into_iter().chain(files.iter().copied()).collect();
    let Some(counts) = git::text(directory, &arguments) else { return files.iter().map(|file| format!("{file} adds lines")).collect() };
    counts.lines().filter_map(|line| line.split_once('\t')).filter(|(added, _)| *added != "0").filter_map(|(_, rest)| rest.split_once('\t')).map(|(_, file)| format!("{file} adds lines")).collect()
}
