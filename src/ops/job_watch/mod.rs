mod core;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use self::core::{Alert, Rows, State, presence, read_registry, step};
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north job-watch run [--registry DIR] [--peers FILE] [--state FILE] [--dry-run]";

struct Options {
    registry: PathBuf,
    peers: PathBuf,
    state: PathBuf,
    dry_run: bool,
}

impl Options {
    fn parse(arguments: &[String]) -> Option<Self> {
        let home = PathBuf::from(env::var("HOME").unwrap_or_default());
        let state_dir = env::var("STATE_DIRECTORY").map_or_else(|_| home.join(".local/state/job-watch"), PathBuf::from);
        let mut options = Self {
            registry: home.join("code/south/main/agents/routines"),
            peers: PathBuf::from("/run/nexus/peers.json"),
            state: state_dir.join("state.json"),
            dry_run: false,
        };
        let mut rest = arguments.iter();
        while let Some(flag) = rest.next() {
            match flag.as_str() {
                "--dry-run" => options.dry_run = true,
                "--registry" => options.registry = rest.next()?.into(),
                "--peers" => options.peers = rest.next()?.into(),
                "--state" => options.state = rest.next()?.into(),
                _ => return None,
            }
        }
        Some(options)
    }
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "run" => match Options::parse(rest) {
            Some(options) => check(&options),
            None => usage(),
        },
        _ => usage(),
    }
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn hosts() -> BTreeMap<String, String> {
    let spec = env::var("JOB_WATCH_HOSTS").unwrap_or_else(|_| "laptop=whiterabbit nexus=nexus".to_owned());
    spec.split_whitespace().filter_map(|pair| pair.split_once('=')).map(|(place, host)| (place.to_owned(), host.to_owned())).collect()
}

fn ledger_rows() -> Result<Rows, String> {
    let north = env::current_exe().map_err(|error| format!("threads job-runs: {error}"))?;
    let output = Command::new(north).args(["threads", "job-runs", "--json"]).output().map_err(|error| format!("threads job-runs: {error}"))?;
    if !output.status.success() {
        return Err(format!("threads job-runs failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let rows: Vec<Value> = serde_json::from_str(if text.trim().is_empty() { "[]" } else { &text }).map_err(|error| format!("threads job-runs: {error}"))?;
    let field = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
    Ok(rows.iter().map(|row| ((field(row, "name"), field(row, "host")), row.clone())).collect())
}

fn send(topic: &str, text: &str, title: &str) -> bool {
    match Command::new("nexus-alert").args([topic, text, title]).output() {
        Ok(output) if output.status.success() => {
            println!("      posted to {topic}: ntfy id {}", String::from_utf8_lossy(&output.stdout).trim());
            true
        }
        Ok(output) => {
            eprintln!("job-watch: nexus-alert {topic} failed: {}", String::from_utf8_lossy(&output.stderr).trim());
            false
        }
        Err(error) => {
            eprintln!("job-watch: nexus-alert {topic} failed: {error}");
            false
        }
    }
}

fn check(options: &Options) -> u8 {
    let now = env::var("JOB_WATCH_NOW").ok().and_then(|value| value.parse().ok()).unwrap_or_else(|| Timestamp::now().unix_seconds_f64());
    let hosts = hosts();
    let entries = read_registry(&options.registry);
    let rows = match ledger_rows() {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("job-watch: {error}");
            return 1;
        }
    };
    let read_json = |path: &PathBuf| fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let peers = read_json(&options.peers).unwrap_or(Value::Null);
    let state: State = read_json(&options.state).and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
    let (alerts, mut next) = step(&entries, &rows, &hosts, &presence(&peers, "laptop", now), &state, now);
    let topic_override = env::var("JOB_WATCH_TOPIC").ok();
    let mut failed = 0;
    for Alert { name, topic, text, title } in &alerts {
        let topic = topic_override.as_deref().unwrap_or(topic);
        println!("{topic}: {title}: {text}");
        if !options.dry_run && !send(topic, text, title) {
            failed += 1;
            match state.get(name) {
                Some(previous) => next.insert(name.clone(), previous.clone()),
                None => next.remove(name),
            };
        }
    }
    if !options.dry_run && let Err(error) = save(options, &next) {
        eprintln!("job-watch: {}: {error}", options.state.display());
        return 1;
    }
    let watched = entries.iter().filter(|entry| hosts.get(&entry.placement).is_some_and(|host| rows.contains_key(&(entry.job.clone(), host.clone())))).count();
    println!("job-watch: {} jobs with deadlines, {watched} with runs, {} alerts, {failed} unsent", entries.len(), alerts.len());
    u8::from(failed > 0)
}

fn save(options: &Options, state: &State) -> std::io::Result<()> {
    if let Some(parent) = options.state.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = options.state.with_extension("tmp");
    fs::write(&temporary, serde_json::to_string_pretty(state).map_err(std::io::Error::other)?)?;
    fs::rename(temporary, &options.state)
}
