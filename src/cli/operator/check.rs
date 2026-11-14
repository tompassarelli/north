//! `north config operator check [DIR] [--only NAME,...]`: every schema-class file of the operator repository read against its typed key table.
//! A schema is one entry of `SCHEMAS`; a finding prints as `FILE: KEY: reason`.
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use crate::error::{NorthError, NorthResult};

pub const USAGE: &str = "usage: north config operator check [DIR] [--only NAME,...] [--distinct-from OWNER]";

mod schemas;
mod walk;

use schemas::{SCHEMAS, Schema};
use walk::walk;

const GROUPS: &[(&str, &[&str])] = &[("policy", &["frozen-topics", "operating-point", "watch", "rules", "private-kinds"])];

fn check_file(schema: &Schema, root: &Path) -> Vec<String> {
    let path = root.join(schema.file);
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => return vec![format!("{}: file: cannot read {}: {error}", schema.file, path.display())],
    };
    let document: DocumentMut = match source.parse() {
        Ok(document) => document,
        Err(error) => return vec![format!("{}: file: {}", schema.file, error.to_string().replace('\n', " "))],
    };
    let mut found = Vec::new();
    walk(schema, "", "", document.as_table(), &mut found);
    if found.is_empty()
        && let Some(values) = schema.values
        && let Err(reason) = values(&source)
    {
        let reason = reason.strip_prefix(&format!("{}: ", schema.file)).unwrap_or(&reason);
        found.push(format!("{}: file: {reason}", schema.file));
    }
    found
}

fn default_root() -> PathBuf {
    crate::platform::orchestration::path().parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default()
}

pub fn findings(root: &Path, only: Option<&[&str]>) -> Vec<String> {
    SCHEMAS.iter().filter(|schema| only.is_none_or(|names| selects(names, schema))).flat_map(|schema| check_file(schema, root)).collect()
}

fn files(only: Option<&[&str]>) -> Vec<&'static str> {
    let mut files: Vec<&str> = SCHEMAS.iter().filter(|schema| only.is_none_or(|names| selects(names, schema))).map(|schema| schema.file).collect();
    files.dedup();
    files
}

fn selects(names: &[&str], schema: &Schema) -> bool {
    names.iter().any(|name| *name == schema.name || GROUPS.iter().any(|(group, members)| group == name && members.contains(&schema.name)))
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let usage = || NorthError::Usage(USAGE.into());
    let (mut root, mut only, mut owner) = (None, None::<Vec<&str>>, None);
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--only" => only = Some(rest.next().ok_or_else(usage)?.split(',').collect()),
            "--distinct-from" => owner = Some(PathBuf::from(rest.next().ok_or_else(usage)?)),
            flag if flag.starts_with("--") => return Err(usage()),
            _ if root.is_none() => root = Some(PathBuf::from(argument)),
            _ => return Err(usage()),
        }
    }
    if let Some(unknown) = only.iter().flatten().find(|name| !SCHEMAS.iter().any(|schema| schema.name == **name) && !GROUPS.iter().any(|(group, _)| *group == **name)) {
        let names: Vec<&str> = SCHEMAS.iter().map(|schema| schema.name).chain(GROUPS.iter().map(|(group, _)| *group)).collect();
        return Err(NorthError::Usage(format!("{USAGE}; NAME {unknown} is not one of {}", names.join(", "))));
    }
    let root = root.unwrap_or_else(default_root);
    let mut found = findings(&root, only.as_deref());
    if let Some(owner) = &owner {
        found.extend(super::same_value::findings(&root, owner, &files(only.as_deref())));
    }
    if found.is_empty() {
        return Ok(());
    }
    for line in &found {
        println!("{line}");
    }
    Err(NorthError::Rejected(format!("{} finding(s) in {}", found.len(), root.display())))
}
