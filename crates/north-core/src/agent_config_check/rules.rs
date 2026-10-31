use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use regex::Regex;
use serde_json::Value;

use super::gate::Verdicts;
use super::roots::Roots;
use crate::platform::{git, toml_json};

fn shown(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn text_of<'a>(rule: &'a Value, key: &str) -> Option<&'a str> {
    rule.get(key).and_then(Value::as_str)
}

fn git_text(directory: &Path, args: &[&str]) -> Option<String> {
    let output = git::run(directory, args).ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn session(entry: &Value, roots: &Roots) -> Result<String, String> {
    let reference = entry.get("ref").and_then(Value::as_str).unwrap_or_default();
    let source = text_of(entry, "source").unwrap_or_default();
    if let Some(session) = text_of(entry, "session").filter(|session| !session.is_empty()) {
        return Ok(session.to_owned());
    }
    if source != "git" && source != "github" {
        return Ok(reference.to_owned());
    }
    let issue = Regex::new(r"([\w.-]+)#(\d+)$").unwrap().captures(reference);
    if let (true, Some(issue)) = (source == "github", issue) {
        return Ok(format!("{}#{}", &issue[1], &issue[2]));
    }
    let (repo, sha) = reference.split_once('@').unwrap_or((reference, ""));
    let root = roots.checkout(repo);
    let shown = git::run(&root, &["show", "-s", "--format=%ae %as%n%B", sha]).ok().filter(|output| output.status.success());
    let Some(shown) = shown else {
        return Ok(format!("{repo} {}", text_of(entry, "ts").unwrap_or("None").chars().take(10).collect::<String>()));
    };
    let stdout = String::from_utf8_lossy(&shown.stdout).into_owned();
    let (head, body) = stdout.split_once('\n').unwrap_or((&stdout, ""));
    let cited = fancy_regex::Regex::new(r"(?<![\w.-])(?:[\w.-]+/)?([\w.-]*)#(\d+)\b").unwrap().captures(body).ok().flatten();
    Ok(match cited {
        Some(cited) => {
            let name = cited.get(1).map_or("", |name| name.as_str());
            format!("{}#{}", if name.is_empty() { repo } else { name }, &cited[2])
        }
        None => format!("{repo} {head}"),
    })
}

fn earned(rules: &[Value], operator: &Path, ledger: &str, roots: &Roots) -> Result<Vec<String>, String> {
    let mut sessions: BTreeMap<String, String> = BTreeMap::new();
    for line in ledger.lines().filter(|line| !line.trim().is_empty()) {
        let entry: Value = serde_json::from_str(line).map_err(|error| format!("incidents.jsonl: {error}"))?;
        sessions.insert(shown(entry.get("id")), session(&entry, roots)?);
    }
    let prefix = git_text(operator, &["rev-parse", "--show-prefix"]);
    let base = if git_text(operator, &["rev-parse", "--verify", "-q", "origin/main"]).is_some() { "origin/main" } else { "HEAD" };
    let landed = prefix.as_ref().and_then(|prefix| git_text(operator, &["show", &format!("{base}:{}rules.toml", prefix.trim())]));
    let Some(landed) = landed else { return Ok(Vec::new()) };
    let landed = toml_json::parse(&landed).map_err(|error| error.to_string())?;
    let landed_ids: BTreeSet<String> = landed["rule"].as_array().into_iter().flatten().map(|rule| shown(rule.get("id"))).collect();
    Ok(rules
        .iter()
        .filter(|rule| !landed_ids.contains(&shown(rule.get("id"))))
        .filter(|rule| {
            let cited: BTreeSet<&String> = rule["incidents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| sessions.get(&shown(Some(id))))
                .filter(|session| !session.is_empty())
                .collect();
            cited.len() < 2
        })
        .map(|rule| format!("rules.toml {}: a new rule must cite incidents.jsonl incidents from at least 2 sessions", shown(rule.get("id"))))
        .collect())
}

type Pair = (Option<String>, Option<String>);

fn bullets_of(agents: &str) -> Vec<Pair> {
    let mut section: Option<String> = None;
    let mut bullets = Vec::new();
    for line in agents.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            section = Some(name.to_owned());
        } else if let Some(text) = line.strip_prefix("- ") {
            bullets.push((section.clone(), Some(text.to_owned())));
        }
    }
    bullets
}

fn placement_of(rule: &Value) -> String {
    text_of(rule, "placement").unwrap_or_default().to_owned()
}

fn placement_errors(rules: &[Value]) -> Vec<String> {
    let ids: Vec<Option<&str>> = rules.iter().map(|rule| text_of(rule, "id").filter(|id| !id.is_empty())).collect();
    let valid = Regex::new(r"^(?:global|dropped|(?:merged|skill|hook):[a-z0-9-]+)$").unwrap();
    let mut errors: Vec<String> = rules
        .iter()
        .filter(|rule| !valid.is_match(&placement_of(rule)))
        .map(|rule| format!("rules.toml {}: placement must be global, merged:<id>, skill:<name>, hook:<name> or dropped", shown(rule.get("id"))))
        .collect();
    for rule in rules {
        if let Some(target) = placement_of(rule).strip_prefix("merged:")
            && !ids.contains(&Some(target))
        {
            errors.push(format!("rules.toml {}: {} names no rules.toml id", shown(rule.get("id")), placement_of(rule)));
        }
    }
    errors
}

fn identity_errors(rules: &[Value]) -> Vec<String> {
    let ids: Vec<Option<&str>> = rules.iter().map(|rule| text_of(rule, "id").filter(|id| !id.is_empty())).collect();
    let distinct: BTreeSet<&Option<&str>> = ids.iter().collect();
    let mut errors = Vec::new();
    if distinct.len() != ids.len() || ids.iter().any(Option::is_none) {
        errors.push("rules.toml ids must be present and unique".to_owned());
    }
    errors.extend(
        rules
            .iter()
            .filter(|rule| text_of(rule, "fixes").unwrap_or_default().trim().is_empty())
            .map(|rule| format!("rules.toml {}: fixes is empty", shown(rule.get("id")))),
    );
    errors
}

fn register_errors(rules: &[Value], bullets: &[Pair]) -> (Vec<String>, usize) {
    let mut errors = placement_errors(rules);
    let registered: Vec<Pair> = rules
        .iter()
        .filter(|rule| placement_of(rule) == "global")
        .map(|rule| (text_of(rule, "section").map(str::to_owned), text_of(rule, "text").map(str::to_owned)))
        .collect();
    let show_pair = |pair: &Pair| format!("[{}] {}", pair.0.as_deref().unwrap_or("None"), pair.1.as_deref().unwrap_or("None"));
    errors.extend(bullets.iter().filter(|pair| !registered.contains(pair)).map(|pair| format!("AGENTS.md bullet has no rules.toml entry: {}", show_pair(pair))));
    errors.extend(registered.iter().filter(|pair| !bullets.contains(pair)).map(|pair| format!("rules.toml entry has no AGENTS.md bullet: {}", show_pair(pair))));
    if errors.is_empty() && registered != bullets {
        errors.push("rules.toml entries are not in AGENTS.md order".into());
    }
    errors.extend(identity_errors(rules));
    (errors, registered.len())
}

pub(super) fn rule_register(operator: &Path, roots: &Roots) -> Result<Verdicts, String> {
    let agents = fs::read_to_string(operator.join("AGENTS.md")).map_err(|error| error.to_string())?;
    let parsed = fs::read_to_string(operator.join("rules.toml")).map_err(|error| error.to_string()).and_then(|body| toml_json::parse(&body).map_err(|error| error.to_string()));
    let rules = match parsed {
        Ok(document) => document["rule"].as_array().cloned().unwrap_or_default(),
        Err(error) => return Ok(Verdicts { text: format!("rules.toml is unreadable: {error}"), passed: false }),
    };
    let (mut errors, registered) = register_errors(&rules, &bullets_of(&agents));
    if let Ok(ledger) = fs::read_to_string(operator.join("incidents.jsonl")) {
        errors.extend(earned(&rules, operator, &ledger, roots)?);
    }
    let text = if errors.is_empty() {
        format!("{registered} global rules match AGENTS.md bullets 1:1, {} placed", rules.len())
    } else {
        errors.join("\n")
    };
    Ok(Verdicts { text, passed: errors.is_empty() })
}
