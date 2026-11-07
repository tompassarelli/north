use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use crate::error::NorthResult;

use super::configuration;
use super::registration_checks::git_text;

const PATTERN: &str = r#"authoring_guards_off\(|const (UNIT|ID): &str = ""#;
const CALL: &str = "authoring_guards_off(";

pub(super) fn require_guard_ids_named(north: &Path, reference: &str, catalog: &Value) -> NorthResult<()> {
    let sources = git_text(north, &["grep", "-n", "-E", PATTERN, reference, "--", "crates", "src"])?;
    let named: BTreeSet<&str> = catalog
        .get("activation")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(unit, _)| unit.as_str())
        .collect();
    let unnamed = unnamed_guard_ids(&sources, reference, &named);
    if unnamed.is_empty() {
        return Ok(());
    }
    configuration(unnamed.join("\n"))
}

fn unnamed_guard_ids(sources: &str, reference: &str, named: &BTreeSet<&str>) -> Vec<String> {
    let mut consts: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::new();
    let mut calls: Vec<(&str, &str, &str)> = Vec::new();
    for entry in sources.lines() {
        let entry = entry.strip_prefix(reference).and_then(|rest| rest.strip_prefix(':')).unwrap_or(entry);
        let mut parts = entry.splitn(3, ':');
        let (Some(path), Some(line), Some(text)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if let Some(rest) = text.trim_start().strip_prefix("const ").or_else(|| text.trim_start().strip_prefix("pub const "))
            && let Some((name, value)) = rest.split_once(": &str = \"")
            && let Some((value, _)) = value.split_once('"')
        {
            consts.entry(path).or_default().insert(name, value);
        }
        if let Some((_, after)) = text.split_once(CALL)
            && let Some((argument, _)) = after.split_once(')')
        {
            calls.push((path, line, argument.trim()));
        }
    }
    let mut unnamed = Vec::new();
    for (path, line, argument) in calls {
        let id = match argument.strip_prefix('"').and_then(|literal| literal.strip_suffix('"')) {
            Some(literal) => literal,
            None => match consts.get(path).and_then(|file| file.get(argument)) {
                Some(value) => value,
                None => continue,
            },
        };
        let id = id.strip_suffix(".sh").or_else(|| id.strip_suffix(".js")).unwrap_or(id);
        if !named.contains(id) {
            unnamed.push(format!("{path}:{line} asks authoring_guards_off for {id}, which no unit in agents/catalog-config.json names, so the guard reads as off on every host (north#722)"));
        }
    }
    unnamed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guard_asking_for_an_id_the_catalog_dropped_is_named() {
        let sources = "REF:crates/north-guard/src/guard/spawn_capacity.rs:19:const UNIT: &str = \"spawn-capacity-guard\";\n\
                       REF:crates/north-guard/src/guard/spawn_capacity.rs:60:    if crate::guard::activation::authoring_guards_off(UNIT) {\n\
                       REF:crates/north-guard/src/guard/native_item.rs:21:    if crate::guard::activation::authoring_guards_off(\"native-item-guard\") {\n\
                       REF:crates/north-guard/src/guard/activation.rs:80:pub fn authoring_guards_off(id: &str) -> bool {";
        let kept: BTreeSet<&str> = ["spawn-capacity-guard", "native-item-guard"].into_iter().collect();
        assert!(unnamed_guard_ids(sources, "REF", &kept).is_empty());
        let dropped: BTreeSet<&str> = ["native-item-guard"].into_iter().collect();
        let lines = unnamed_guard_ids(sources, "REF", &dropped);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("crates/north-guard/src/guard/spawn_capacity.rs:60 asks authoring_guards_off for spawn-capacity-guard"), "{lines:?}");
    }
}
