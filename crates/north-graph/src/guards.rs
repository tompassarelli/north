use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::query::parse_query;
use crate::store::Graph;
use crate::term::Term;

const RULES: &str = "graph-rules";

pub struct Finding {
    pub guard: String,
    pub row: Vec<Term>,
}

pub struct Verdict {
    pub guards: usize,
    pub findings: Vec<Finding>,
}

pub fn check(graph: &Graph, directory: &Path) -> Result<Verdict, String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX));
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .map_err(|why| format!("{}: {why}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect();
    paths.sort();
    let rules = directory.parent().map_or_else(|| Path::new(RULES).to_path_buf(), |parent| parent.join(RULES));
    let mut verdict = Verdict { guards: paths.len(), findings: Vec::new() };
    for path in paths {
        let guard = path.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let text = expand(&read(&path)?, &rules, now).map_err(|why| format!("guard {guard}: {why}"))?;
        let query = parse_query(&text).map_err(|why| format!("guard {guard}: {why}"))?;
        let answer = graph.ask(&query).map_err(|why| format!("guard {guard}: {why}"))?;
        verdict.findings.extend(answer.rows.into_iter().map(|row| Finding { guard: guard.clone(), row }));
    }
    Ok(verdict)
}

fn read(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|why| format!("{}: {why}", path.display()))?;
    serde_json::from_str(&text).map_err(|why| format!("{}: {why}", path.display()))
}

fn strata(query: &mut Value) -> Vec<Value> {
    match (query.get_mut("strata").map(Value::take), query.get_mut("rules").map(Value::take)) {
        (Some(Value::Array(strata)), _) => strata,
        (_, Some(rules)) => vec![rules],
        _ => Vec::new(),
    }
}

fn expand(guard: &Value, rules: &Path, now: i64) -> Result<String, String> {
    let mut guard = guard.clone();
    let uses = guard.as_object_mut().and_then(|fields| fields.remove("uses")).unwrap_or(Value::Null);
    let mut layered = Vec::new();
    for name in uses.as_array().into_iter().flatten() {
        let name = name.as_str().ok_or_else(|| format!("uses names {name}, not a rule file"))?;
        layered.extend(strata(&mut read(&rules.join(format!("{name}.json")))?));
    }
    if !layered.is_empty() {
        layered.extend(strata(&mut guard));
        if let Some(fields) = guard.as_object_mut() {
            fields.remove("rules");
            fields.insert("strata".to_owned(), Value::Array(layered));
        }
    }
    resolve_ago(&mut guard, now)?;
    Ok(guard.to_string())
}

fn resolve_ago(value: &mut Value, now: i64) -> Result<(), String> {
    match value {
        Value::Object(fields) if fields.len() == 1 && fields.contains_key("ago") => {
            let seconds = fields["ago"].as_i64().ok_or_else(|| format!("ago takes whole seconds, not {}", fields["ago"]))?;
            *value = json!(["instant", now.saturating_sub(seconds), 0]);
            Ok(())
        }
        Value::Object(fields) => fields.values_mut().try_for_each(|field| resolve_ago(field, now)),
        Value::Array(items) => items.iter_mut().try_for_each(|item| resolve_ago(item, now)),
        _ => Ok(()),
    }
}
