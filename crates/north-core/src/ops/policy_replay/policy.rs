use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table};

use super::pyfmt::{list_repr, lstrip, splitlines, str_repr, strip};
use crate::error::NorthError;

fn refusal(message: String) -> NorthError {
    NorthError::Usage(format!("policy-replay: {message}"))
}

fn expand_user(path: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix("~/") {
        Some(tail) => Path::new(&home).join(tail),
        None if path == "~" => PathBuf::from(home),
        None => PathBuf::from(path),
    }
}

fn read(path: &Path) -> Result<String, NorthError> {
    fs::read_to_string(path).map_err(NorthError::Io)
}

pub fn policy_for_arm(text: &str, minus: &[String]) -> Result<String, NorthError> {
    let mut prefixes: Vec<&str> = Vec::new();
    for prefix in minus {
        if !prefixes.contains(&prefix.as_str()) {
            prefixes.push(prefix);
        }
    }
    let mut hits = vec![0usize; prefixes.len()];
    let mut kept = String::new();
    for line in splitlines(text, true) {
        let rule = lstrip(line).strip_prefix("- ");
        match rule.and_then(|body| prefixes.iter().position(|prefix| body.starts_with(prefix))) {
            Some(index) => hits[index] += 1,
            None => kept.push_str(line),
        }
    }
    let missing: Vec<String> = prefixes.iter().zip(&hits).filter(|(_, hit)| **hit == 0).map(|(prefix, _)| (*prefix).to_owned()).collect();
    if missing.is_empty() { Ok(kept) } else { Err(refusal(format!("rule prefix matches no rule: {}", list_repr(&missing)))) }
}

pub fn prefixes_from_file(path: &Path) -> Result<Vec<String>, NorthError> {
    let text = read(path)?;
    Ok(splitlines(&text, false)
        .into_iter()
        .map(strip)
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix("- ").map_or(line, strip).to_owned())
        .collect())
}

pub fn arm_minus(arm: &str, scenario: &Value) -> Result<Vec<String>, NorthError> {
    if arm == "full" {
        return Ok(Vec::new());
    }
    if let Some(list) = arm.strip_prefix("minus:") {
        return Ok(list.split(',').map(strip).filter(|prefix| !prefix.is_empty()).map(str::to_owned).collect());
    }
    if let Some(path) = arm.strip_prefix("minus-file:") {
        return prefixes_from_file(&expand_user(path));
    }
    match scenario.get("arms").and_then(|arms| arms.get(arm)).and_then(Value::as_array) {
        Some(prefixes) => Ok(prefixes.iter().filter_map(Value::as_str).map(str::to_owned).collect()),
        None => Err(refusal(format!("unknown arm {} for {}", str_repr(arm), scenario.get("id").and_then(Value::as_str).unwrap_or("None")))),
    }
}

pub fn no_fable(names: &[Option<&str>]) -> Result<(), NorthError> {
    if names.iter().flatten().any(|name| name.to_lowercase().contains("fable")) {
        return Err(refusal("Fable is never the main or advisor model here".into()));
    }
    Ok(())
}

pub fn strip_frontmatter(text: &str) -> &str {
    let frontmatter = Regex::new(r"(?s)\A---\n.*?\n---\n").expect("frontmatter pattern compiles");
    frontmatter.find(text).map_or(text, |found| &text[found.end()..])
}

#[derive(Debug, PartialEq)]
pub struct Placement {
    pub rule: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Variant {
    pub name: String,
    pub policy: Option<PathBuf>,
    pub minus: Vec<String>,
    pub minus_file: Vec<PathBuf>,
    pub placement: Vec<Placement>,
    pub plus: Vec<PathBuf>,
}

fn strings(table: &Table, key: &str) -> Vec<String> {
    table.get(key).and_then(Item::as_array).map(|list| list.iter().filter_map(|v| v.as_str()).map(str::to_owned).collect()).unwrap_or_default()
}

impl Variant {
    pub fn parse_all(text: &str) -> Result<Vec<Variant>, NorthError> {
        let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| refusal(format!("variants file does not parse: {error}")))?;
        let tables = document.get("variant").and_then(Item::as_array_of_tables).ok_or_else(|| refusal("variants file has no [[variant]] table".into()))?;
        tables
            .iter()
            .map(|table| {
                let name = table.get("name").and_then(Item::as_str).ok_or_else(|| refusal("a [[variant]] has no name".into()))?;
                let placement = table.get("placement").and_then(Item::as_array).map(|moves| {
                    moves.iter().filter_map(|m| m.as_inline_table()).map(|m| Placement {
                        rule: m.get("rule").and_then(|v| v.as_str()).map(str::to_owned),
                        to: m.get("to").and_then(|v| v.as_str()).map(str::to_owned),
                    }).collect()
                });
                Ok(Variant {
                    name: name.to_owned(),
                    policy: table.get("policy").and_then(Item::as_str).filter(|path| !path.is_empty()).map(expand_user),
                    minus: strings(table, "minus"),
                    minus_file: strings(table, "minus_file").iter().map(|path| expand_user(path)).collect(),
                    placement: placement.unwrap_or_default(),
                    plus: strings(table, "plus").iter().map(|path| expand_user(path)).collect(),
                })
            })
            .collect()
    }
}

pub fn variant_policy(base: &str, variant: &Variant) -> Result<String, NorthError> {
    let base = match &variant.policy {
        Some(path) => read(path)?,
        None => base.to_owned(),
    };
    let mut minus = variant.minus.clone();
    for path in &variant.minus_file {
        minus.extend(prefixes_from_file(path)?);
    }
    for placed in &variant.placement {
        let rule = placed.rule.clone();
        if !matches!(placed.to.as_deref(), Some("skill" | "hook")) {
            return Err(refusal(format!("placement of {} needs to = skill or hook", rule.as_deref().map_or("None".into(), str_repr))));
        }
        minus.extend(rule);
    }
    let mut text = policy_for_arm(&base, &minus)?;
    for path in &variant.plus {
        let addition = read(path)?;
        text = format!("{}\n\n{}\n", text.trim_end_matches('\n'), strip_frontmatter(&addition).trim_matches('\n'));
    }
    Ok(text)
}
