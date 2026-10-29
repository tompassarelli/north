use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;
use toml_edit::{DocumentMut, Item};

use super::pyfmt::{float_repr, json_string, list_repr};
use super::rows::{Scenarios, VariantRow};
use super::stats::{Verdict, compare, covered, flips};
use crate::error::NorthError;

const POINT_KEYS: [&str; 8] = ["date", "model", "variant", "policy_sha256", "startup_tokens", "conformance", "n", "evidence"];

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Scalar {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Scalar {
    fn from_item(item: &Item) -> Option<Scalar> {
        let value = item.as_value()?;
        value
            .as_str()
            .map(|text| Scalar::Str(text.to_owned()))
            .or_else(|| value.as_integer().map(Scalar::Int))
            .or_else(|| value.as_float().map(Scalar::Float))
            .or_else(|| value.as_bool().map(Scalar::Bool))
    }

    fn toml(&self) -> String {
        match self {
            Scalar::Str(text) => json_string(text),
            Scalar::Int(number) => number.to_string(),
            Scalar::Float(number) => float_repr(*number),
            Scalar::Bool(flag) => flag.to_string(),
        }
    }

    fn display(&self) -> String {
        match self {
            Scalar::Str(text) => text.clone(),
            Scalar::Bool(flag) => if *flag { "True" } else { "False" }.to_owned(),
            other => other.toml(),
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Scalar::Int(number) => Some(*number as f64),
            Scalar::Float(number) => Some(*number),
            _ => None,
        }
    }

    fn count(&self) -> usize {
        self.number().map_or(0, |number| number as usize)
    }
}

fn shown(scalar: Option<&Scalar>) -> String {
    scalar.map_or_else(|| "None".to_owned(), Scalar::display)
}

fn refused(message: String) -> NorthError {
    NorthError::Refused(format!("policy-replay: {message}"))
}

pub(crate) fn write_candidate(path: &Path, row: &VariantRow, evidence: &Path, provisional: bool, date: &str) -> Result<(), NorthError> {
    let values = [
        Scalar::Str(date.to_owned()),
        Scalar::Str(row.model.clone().unwrap_or_default()),
        Scalar::Str(row.variant.clone()),
        Scalar::Str(row.policy_sha256.clone()),
        Scalar::Int(row.startup_tokens as i64),
        Scalar::Float(row.conformance.unwrap_or(0.0)),
        Scalar::Int(row.n as i64),
        Scalar::Str(evidence.display().to_string()),
    ];
    let mut lines: Vec<String> = POINT_KEYS.iter().zip(&values).map(|(key, value)| format!("{key} = {}", value.toml())).collect();
    if provisional {
        lines.push("provisional = true".into());
    }
    lines.push(String::new());
    lines.push("[scenarios]".into());
    lines.extend(row.scenarios.iter().map(|(scenario, [passed, n])| format!("{} = [{passed}, {n}]", json_string(scenario))));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(NorthError::Io)?;
    }
    fs::write(path, lines.join("\n") + "\n").map_err(NorthError::Io)
}

pub(crate) fn promoted_text(current: &str, values: &[(&str, Scalar)], provisional: bool) -> String {
    let key = Regex::new(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=").expect("key pattern compiles");
    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for line in super::pyfmt::splitlines(current, false) {
        let name = key.captures(line).map(|caps| caps.get(1).map_or("", |m| m.as_str()));
        match name.and_then(|name| values.iter().find(|(known, _)| *known == name)) {
            Some((name, value)) => {
                out.push(format!("{name} = {}", value.toml()));
                seen.push(name);
            }
            None if name == Some("provisional") => {}
            None => out.push(line.to_owned()),
        }
    }
    let mut head: Vec<String> = values.iter().filter(|(name, _)| !seen.contains(name)).map(|(name, value)| format!("{name} = {}", value.toml())).collect();
    if provisional {
        head.push("provisional = true".into());
    }
    head.extend(out);
    head.join("\n") + "\n"
}

fn parse_toml(text: &str, what: &str) -> Result<DocumentMut, NorthError> {
    text.parse().map_err(|error: toml_edit::TomlError| refused(format!("{what} does not parse: {error}")))
}

fn scenario_cells(document: &DocumentMut) -> Scenarios {
    let Some(table) = document.get("scenarios").and_then(Item::as_table_like) else { return Scenarios::new() };
    table
        .iter()
        .filter_map(|(name, item)| {
            let cells = item.as_array()?;
            let count = |at: usize| cells.get(at).and_then(|v| v.as_integer()).map(|v| v as usize);
            Some((name.to_owned(), [count(0)?, count(1)?]))
        })
        .collect()
}

fn point_scenarios(current: &DocumentMut) -> Scenarios {
    let read = |key: &str| current.get(key).and_then(Scalar::from_item);
    let evidence = match read("evidence") {
        Some(Scalar::Str(path)) => PathBuf::from(path),
        _ => PathBuf::new(),
    };
    let sha = read("policy_sha256");
    let Ok(text) = fs::read_to_string(&evidence) else { return Scenarios::new() };
    let mut found = Scenarios::new();
    for line in text.lines() {
        let Ok(Value::Object(row)) = serde_json::from_str::<Value>(line) else { continue };
        let scenarios: Scenarios = row
            .get("scenarios")
            .and_then(Value::as_object)
            .map(|cells| {
                cells
                    .iter()
                    .filter_map(|(name, cell)| {
                        let cell = cell.as_array()?;
                        Some((name.clone(), [cell.first()?.as_u64()? as usize, cell.get(1)?.as_u64()? as usize]))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let row_sha = row.get("policy_sha256").and_then(Value::as_str);
        let point_sha = match &sha {
            Some(Scalar::Str(text)) => Some(text.as_str()),
            _ => None,
        };
        if row_sha == point_sha && !scenarios.is_empty() && covered(&scenarios) == scenarios.len() {
            found = scenarios;
        }
    }
    found
}

pub(crate) struct PromoteArgs<'a> {
    pub(crate) candidate: &'a Path,
    pub(crate) scenarios: &'a [PathBuf],
    pub(crate) operating_point: Option<&'a Path>,
    pub(crate) accept_drop: Option<&'a str>,
}

fn operating_point_path(args: &PromoteArgs) -> Result<PathBuf, NorthError> {
    if let Some(path) = args.operating_point {
        return Ok(path.to_path_buf());
    }
    let first = args.scenarios.first().ok_or_else(|| refused("promote needs at least one scenario file".into()))?;
    let resolved = fs::canonicalize(first).map_err(NorthError::Io)?;
    Ok(resolved.parent().and_then(Path::parent).unwrap_or(Path::new("/")).join("operating-point.toml"))
}

fn required_ids(scenarios: &[PathBuf]) -> Result<Vec<String>, NorthError> {
    let mut ids = Vec::new();
    for path in scenarios {
        let text = fs::read_to_string(path).map_err(NorthError::Io)?;
        let scenario: Value = serde_json::from_str(&text).map_err(NorthError::Json)?;
        let id = scenario.get("id").and_then(Value::as_str).ok_or_else(|| refused(format!("{} has no id", path.display())))?;
        ids.push(id.to_owned());
    }
    ids.sort();
    Ok(ids)
}

fn needed(candidate: &DocumentMut, key: &str) -> Result<Scalar, NorthError> {
    candidate.get(key).and_then(Scalar::from_item).ok_or_else(|| refused(format!("candidate has no {key}")))
}

fn drop_refusals(current: &DocumentMut, anchor: &[(&str, Option<Scalar>); 3], conformance: &Scalar, n: &Scalar) -> Vec<String> {
    let read = |key: &str| current.get(key).and_then(Scalar::from_item);
    let anchor_name = format!("anchor of {}", shown(anchor[2].1.as_ref()));
    let points = [("current", read("conformance"), read("n")), (anchor_name.as_str(), anchor[0].1.clone(), anchor[1].1.clone())];
    let candidate = (conformance.number().unwrap_or(0.0), n.count());
    points
        .iter()
        .filter(|(_, c, count)| {
            c.as_ref().and_then(Scalar::number).is_some_and(|c| count.as_ref().is_some_and(|n| n.count() != 0) && compare((c, count.as_ref().map_or(0, Scalar::count)), candidate) == Verdict::Drop)
        })
        .map(|(name, c, count)| format!("is significantly below the {name} {} (n={})", shown(c.as_ref()), shown(count.as_ref())))
        .collect()
}

pub(crate) fn promote(args: &PromoteArgs) -> Result<String, NorthError> {
    let candidate_text = fs::read_to_string(args.candidate).map_err(NorthError::Io)?;
    let candidate = parse_toml(&candidate_text, "candidate")?;
    let offered = scenario_cells(&candidate);
    let missing: Vec<String> = required_ids(args.scenarios)?.into_iter().filter(|id| offered.get(id).is_none_or(|cell| cell[1] == 0)).collect();
    if !missing.is_empty() {
        return Err(refused(format!("refuse promote: candidate does not cover {}", list_repr(&missing))));
    }
    let target = operating_point_path(args)?;
    let current_text = if target.exists() { fs::read_to_string(&target).map_err(NorthError::Io)? } else { String::new() };
    let current = parse_toml(&current_text, "operating point")?;
    let read = |key: &str| current.get(key).and_then(Scalar::from_item);
    let (conformance, n, date) = (needed(&candidate, "conformance")?, needed(&candidate, "n")?, needed(&candidate, "date")?);
    let mut anchor: Vec<(&str, Option<Scalar>)> = vec![
        ("anchor_conformance", read("anchor_conformance").or_else(|| read("conformance"))),
        ("anchor_n", read("anchor_n").or_else(|| read("n"))),
        ("anchor_date", read("anchor_date").or_else(|| read("date"))),
    ];
    if let Some(reason) = args.accept_drop.filter(|reason| !reason.is_empty()) {
        anchor = vec![
            ("anchor_conformance", Some(conformance.clone())),
            ("anchor_n", Some(n.clone())),
            ("anchor_date", Some(date.clone())),
            ("accepted_drop", Some(Scalar::Str(reason.to_owned()))),
            ("accepted_drop_date", Some(date.clone())),
        ];
    } else {
        let basis: [(&str, Option<Scalar>); 3] = [anchor[0].clone(), anchor[1].clone(), anchor[2].clone()];
        let mut refusals = drop_refusals(&current, &basis, &conformance, &n);
        let flipped = flips(&point_scenarios(&current), &offered);
        if !flipped.is_empty() {
            refusals.push(format!("flips {} to 0 passes where the current point passed every trial", flipped.join(", ")));
        }
        if !refusals.is_empty() {
            return Err(refused(format!(
                "refuse promote: candidate conformance {} (n={}) {}; pass --accept-drop REASON to record an accepted drop",
                conformance.display(),
                n.display(),
                refusals.join("; ")
            )));
        }
    }
    let mut values: Vec<(&str, Scalar)> = Vec::new();
    for key in POINT_KEYS {
        values.push((key, needed(&candidate, key)?));
    }
    values.extend(anchor.into_iter().filter_map(|(name, value)| value.map(|value| (name, value))));
    let provisional = candidate.get("provisional").and_then(Item::as_bool).unwrap_or(false);
    fs::write(&target, promoted_text(&current_text, &values, provisional)).map_err(NorthError::Io)?;
    Ok(format!(
        "promoted {} -> {}: conformance {} -> {}, startup tokens {} -> {}",
        shown(candidate.get("variant").and_then(Scalar::from_item).as_ref()),
        target.display(),
        shown(read("conformance").as_ref()),
        conformance.display(),
        shown(read("startup_tokens").as_ref()),
        shown(candidate.get("startup_tokens").and_then(Scalar::from_item).as_ref()),
    ))
}
