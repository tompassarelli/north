use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use super::accounts::{config_file, default_home};

mod landing;
mod outcome;
mod row;
mod telemetry;

use landing::MainIndex;
use telemetry::Prices;

const USAGE: &str = "Usage: north ledger claude-runs --settled FILE --recorded FILE [--session ID] [--since DATE]";
const CATALOG: &str = include_str!("../../../../harness/model-catalog.toml");
const DEFAULTS: &str = include_str!("../../../../harness/defaults.toml");
const DAY: Duration = Duration::from_secs(86_400);

pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(entries) => !entries.is_empty(),
    }
}

pub(super) fn present(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
}

pub(super) fn int(value: &Value) -> i64 {
    value.as_i64().or_else(|| value.as_f64().map(|number| number as i64)).unwrap_or(0)
}

pub(super) fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9e15 { json!(value as i64) } else { json!(value) }
}

struct Options {
    session: Option<String>,
    since: String,
    settled: HashSet<String>,
    recorded: HashSet<String>,
}

const UNLABELLED: &str = "claude";

struct Root {
    projects: PathBuf,
    account: String,
}

struct FleetSession {
    role: &'static str,
    effort: Option<String>,
}

struct Ledger {
    policy: Option<DocumentMut>,
    prices: Prices,
    main: MainIndex,
    sessions: HashMap<String, FleetSession>,
    now: Timestamp,
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let options = parse(arguments)?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or_else(|| NorthError::Usage("no home directory".into()))?);
    let policy = fs::read_to_string(crate::pace::target::policy_path()).ok().and_then(|text| text.parse().ok());
    let main = std::env::var_os("LEDGER_MAIN").and_then(|path| fs::read_to_string(path).ok()).unwrap_or_default();
    let ledger = Ledger { policy, prices: Prices::parse(CATALOG, DEFAULTS), main: MainIndex::parse(&main), sessions: fleet_sessions(), now: Timestamp::now() };
    let explicit = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|path| !path.is_empty()).map(PathBuf::from);
    let mut out = BufWriter::new(io::stdout().lock());
    for root in roots(&home, explicit, ledger.policy.as_ref(), &crate::platform::host::name()) {
        for path in transcripts(&root.projects, &options) {
            if let Some(row) = record(&ledger, &options, &root, &path) {
                writeln!(out, "{}", Value::Object(row))?;
            }
        }
    }
    Ok(out.flush()?)
}

fn parse(arguments: &[String]) -> NorthResult<Options> {
    let mut options = Options { session: None, since: String::new(), settled: HashSet::new(), recorded: HashSet::new() };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let value = words.next().ok_or_else(|| NorthError::Usage(USAGE.into()))?;
        let ids = || -> NorthResult<HashSet<String>> { Ok(fs::read_to_string(value)?.lines().filter(|id| !id.is_empty()).map(str::to_owned).collect()) };
        match word.as_str() {
            "--session" => options.session = Some(value.clone()).filter(|session| !session.is_empty()),
            "--since" => options.since = value.clone(),
            "--settled" => options.settled = ids()?,
            "--recorded" => options.recorded = ids()?,
            _ => return Err(NorthError::Usage(USAGE.into())),
        }
    }
    Ok(options)
}

fn fleet_sessions() -> HashMap<String, FleetSession> {
    let north = std::env::var("NORTH_BIN").ok().filter(|path| !path.is_empty()).unwrap_or_else(|| "north".into());
    let Ok(output) = Command::new(north).args(["fleet", "list", "--json", "--all"]).output() else { return HashMap::new() };
    let nodes: Vec<Value> = serde_json::from_slice(&output.stdout).ok().filter(|_| output.status.success()).unwrap_or_default();
    let mut sessions = HashMap::new();
    for node in &nodes {
        let Some(conversation) = node["conversation"].as_str() else { continue };
        let role = if truthy(&node["parent"]) || truthy(&node["task_ref"]) {
            "worker"
        } else if nodes.iter().any(|other| other["parent"] == node["name"]) {
            "parent"
        } else {
            "proxy"
        };
        let effort = node["effort"].as_str().map(str::to_owned);
        sessions.entry(conversation.to_owned()).or_insert(FleetSession { role, effort });
    }
    sessions
}

fn roots(home: &Path, explicit: Option<PathBuf>, policy: Option<&DocumentMut>, host: &str) -> Vec<Root> {
    let default = default_home(home);
    let mut configs = vec![(explicit.clone().unwrap_or_else(|| default.clone()), explicit, None)];
    let profiles = claude_policy(policy, "profiles").and_then(|profiles| profiles.as_table_like());
    for (label, hosts) in profiles.into_iter().flat_map(|table| table.iter()) {
        let Some(profile) = hosts.get(host) else { continue };
        let path = profile.get("path").and_then(|path| path.as_str()).map(|path| path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest)));
        let token_label = path.as_ref().and(profile.get("token_file")).map(|_| label);
        configs.push((path.clone().unwrap_or_else(|| default.clone()), path, token_label));
    }
    let mut roots: Vec<Root> = Vec::new();
    for (root, config, token_label) in configs {
        let projects = root.join("projects");
        let projects = fs::canonicalize(&projects).unwrap_or(projects);
        let account = account(home, config.as_deref(), policy, token_label);
        match roots.iter_mut().find(|seen| seen.projects == projects) {
            Some(seen) if seen.account == UNLABELLED => seen.account = account,
            Some(_) => {}
            None => roots.push(Root { projects, account }),
        }
    }
    roots
}

fn claude_policy<'a>(policy: Option<&'a DocumentMut>, key: &str) -> Option<&'a Item> {
    policy?.get("accounts")?.get("claude")?.get(key)
}

fn account(home: &Path, config: Option<&Path>, policy: Option<&DocumentMut>, token_label: Option<&str>) -> String {
    let default = default_home(home);
    let cache: Value = fs::read_to_string(config_file(config.unwrap_or(&default), &default)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let uuid = [&cache["oauthAccount"]["accountUuid"], &cache["cachedUsageUtilization"]["accountUuid"]].into_iter().find_map(|uuid| uuid.as_str().filter(|uuid| !uuid.is_empty()));
    let identity = uuid.map(|uuid| format!("{:x}", Sha256::digest(uuid.as_bytes()))[..8].to_owned());
    let label = identity.and_then(|identity| claude_policy(policy, "labels")?.get(&identity)?.as_str().map(str::to_owned)).or_else(|| token_label.map(str::to_owned));
    label.map_or_else(|| UNLABELLED.to_owned(), |label| format!("claude-{label}"))
}

fn transcripts(projects: &Path, options: &Options) -> Vec<PathBuf> {
    let cutoff = Timestamp::parse_loose(&options.since).map_or_else(
        || SystemTime::now() - DAY,
        |since| UNIX_EPOCH + Duration::from_secs(since.unix_seconds().max(0) as u64),
    );
    let fresh = |path: &Path| match &options.session {
        Some(_) => true,
        None => fs::metadata(path).and_then(|meta| meta.modified()).is_ok_and(|modified| modified > cutoff),
    };
    let named = |path: &Path, wanted: &str| path.file_name().and_then(|name| name.to_str()) == Some(wanted);
    let mut found = Vec::new();
    for project in entries(projects) {
        for path in entries(&project) {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
            let top = match &options.session {
                Some(session) => name == format!("{session}.jsonl"),
                None => name.ends_with(".jsonl"),
            };
            if top && path.is_file() && fresh(&path) {
                found.push(path.clone());
            }
            if options.session.as_deref().is_some_and(|session| !named(&path, session)) {
                continue;
            }
            for agent in entries(&path.join("subagents")) {
                let name = agent.file_name().and_then(|name| name.to_str()).unwrap_or("");
                if name.starts_with("agent-") && name.ends_with(".jsonl") && agent.is_file() && fresh(&agent) {
                    found.push(agent);
                }
            }
        }
    }
    found.sort();
    let id = |path: &PathBuf| transcript_id(path);
    found.into_iter().filter(|path| !options.settled.contains(&id(path))).collect()
}

fn entries(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory).map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect()).unwrap_or_default()
}

fn transcript_id(path: &Path) -> String {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let name = name.strip_suffix(".jsonl").unwrap_or(name);
    name.strip_prefix("agent-").unwrap_or(name).to_owned()
}

fn record(ledger: &Ledger, options: &Options, root: &Root, path: &Path) -> Option<Map<String, Value>> {
    let id = transcript_id(path);
    let metadata: Option<Value> = fs::read_to_string(path.with_extension("meta.json")).ok().and_then(|text| serde_json::from_str(&text).ok());
    let session = ledger.sessions.get(&id);
    let role = match session {
        Some(session) => session.role,
        None if metadata.as_ref().and_then(|metadata| metadata["agentType"].as_str()).is_some_and(|agent| agent.starts_with("worker")) => "worker",
        None => return None,
    };
    let text = fs::read_to_string(path).ok()?;
    let lines: Vec<Value> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).filter(Value::is_object).collect();
    let mut usage = telemetry::run_usage(&lines, &root.account, &ledger.prices);
    if let Some(metadata) = &metadata {
        usage.insert("effort".into(), effort(ledger.policy.as_ref(), metadata, &usage));
    } else if let Some(effort) = session.and_then(|session| session.effort.as_ref()) {
        usage.insert("effort".into(), json!(effort));
    }
    let tier = if role == "worker" { worker_tier(ledger.policy.as_ref(), metadata.as_ref(), &usage)? } else { String::new() };
    let transcript = row::Transcript { lines: &lines, id: &id, role, tier: &tier, since: &options.since, backfill: options.recorded.contains(&id), now: ledger.now };
    let mut row = row::build(&transcript)?;
    row.extend(landing::checks(&lines, &ledger.main));
    row.extend(telemetry::context_metrics(&lines));
    row.extend(usage);
    row.insert("role".into(), json!(role));
    if !matches!(row["status"].as_str(), Some("running" | "stale")) {
        if path.strip_prefix(&root.projects).is_ok_and(|rest| rest.to_string_lossy().starts_with("-tmp-")) {
            row.insert("role".into(), json!("probe"));
        } else if row["brief"].as_str().is_some_and(|brief| brief.starts_with("[routine:")) {
            row.insert("role".into(), json!("routine"));
        }
    }
    Some(row)
}

fn worker_tier(policy: Option<&DocumentMut>, metadata: Option<&Value>, usage: &Map<String, Value>) -> Option<String> {
    let model = usage["model"].as_str().unwrap_or("unknown");
    let effort = usage["effort"].as_str();
    let agent = match metadata.map(|metadata| &metadata["agentType"]) {
        Some(Value::String(agent)) => Some(agent.clone()),
        Some(other) if truthy(other) => Some(other.to_string()),
        _ => tiers(policy)
            .find(|tier| tier.get("model").and_then(Item::as_str) == Some(model) && tier.get("effort").and_then(Item::as_str) == effort)
            .and_then(|tier| tier.get("agent")?.as_str().map(str::to_owned)),
    };
    let Some(agent) = agent else {
        return Some(if model.contains("haiku") { "haiku".to_owned() } else { format!("{model} {}", effort.unwrap_or("default")) });
    };
    let rest = agent.strip_prefix("worker")?;
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    Some(if model.contains("haiku") { "haiku" } else if rest.is_empty() { "medium" } else { rest }.to_owned())
}

fn tiers(policy: Option<&DocumentMut>) -> impl Iterator<Item = &dyn toml_edit::TableLike> {
    ["tiers", "on_request"]
        .into_iter()
        .filter_map(move |key| policy?.get(key)?.as_table_like())
        .flat_map(|table| table.iter().filter_map(|(_, tier)| tier.as_table_like()))
}

fn effort(policy: Option<&DocumentMut>, metadata: &Value, usage: &Map<String, Value>) -> Value {
    if truthy(&metadata["effort"]) {
        return metadata["effort"].clone();
    }
    let agent = metadata["agentType"].as_str();
    let model = usage["model"].as_str();
    for tier in tiers(policy) {
        if tier.get("agent").and_then(Item::as_str) == agent && tier.get("model").and_then(Item::as_str) == model {
            return tier.get("effort").and_then(|effort| effort.as_str()).map_or(Value::Null, Value::from);
        }
    }
    usage["effort"].clone()
}

#[cfg(test)]
mod tests;
