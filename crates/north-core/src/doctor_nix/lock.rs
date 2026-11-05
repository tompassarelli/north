use serde_json::Value;
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

pub struct Second {
    pub node: String,
    pub reference: String,
    pub rev: String,
    pub pulled_by: Vec<String>,
}

pub struct Allow {
    pub node: String,
    pub rev: Option<String>,
}

fn is_nixpkgs(key: &str, node: &Value) -> bool {
    key.starts_with("nixpkgs") || node["locked"]["repo"] == "nixpkgs"
}

pub fn second_nixpkgs(lock: &Value) -> Vec<Second> {
    let Some(nodes) = lock["nodes"].as_object() else {
        return Vec::new();
    };
    let declared: Vec<&str> = lock["nodes"]["root"]["inputs"].as_object().into_iter().flatten().filter_map(|(_, target)| target.as_str()).collect();
    let mut pulled: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (parent, node) in nodes {
        for target in node["inputs"].as_object().into_iter().flatten().filter_map(|(_, target)| target.as_str()) {
            pulled.entry(target).or_default().push(parent.clone());
        }
    }
    nodes
        .iter()
        .filter(|(key, node)| is_nixpkgs(key, node) && !declared.contains(&key.as_str()))
        .map(|(key, node)| Second {
            node: key.clone(),
            reference: node["original"]["ref"].as_str().unwrap_or_default().to_owned(),
            rev: node["locked"]["rev"].as_str().unwrap_or_default().to_owned(),
            pulled_by: pulled.get(key.as_str()).cloned().unwrap_or_default(),
        })
        .collect()
}

pub fn allowlist(text: &str) -> Result<Vec<Allow>, String> {
    let doc: DocumentMut = text.parse().map_err(|error| format!("allowlist is not TOML: {error}"))?;
    let mut allowed = Vec::new();
    for entry in doc.get("allow").and_then(|allow| allow.as_array_of_tables()).into_iter().flatten() {
        let text = |key: &str| entry.get(key).and_then(|value| value.as_str()).map(str::to_owned);
        let (Some(node), Some(reason)) = (text("node"), text("reason")) else {
            return Err("an [[allow]] entry needs node and reason".into());
        };
        if reason.trim().is_empty() {
            return Err(format!("[[allow]] entry for {node} has an empty reason"));
        }
        allowed.push(Allow { node, rev: text("rev") });
    }
    Ok(allowed)
}

pub fn refusals<'a>(seconds: &'a [Second], allowed: &[Allow]) -> Vec<&'a Second> {
    seconds.iter().filter(|second| !allowed.iter().any(|allow| allow.node == second.node && allow.rev.as_ref().is_none_or(|rev| *rev == second.rev))).collect()
}

pub fn describe(second: &Second) -> String {
    let parents = match second.pulled_by.is_empty() {
        true => "no input".to_owned(),
        false => second.pulled_by.join(", "),
    };
    format!("transitive nixpkgs node `{}` ({} {}), pulled in by {parents}", second.node, second.reference, second.rev.chars().take(8).collect::<String>())
}

pub fn check(lock_text: &str, allow_text: Option<&str>) -> Result<Vec<String>, String> {
    let lock: Value = serde_json::from_str(lock_text).map_err(|error| format!("flake.lock is not JSON: {error}"))?;
    let allowed = allow_text.map(allowlist).transpose()?.unwrap_or_default();
    let seconds = second_nixpkgs(&lock);
    Ok(refusals(&seconds, &allowed).into_iter().map(describe).collect())
}
