use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use super::WatchContext;

const DEFAULT_REPOS: [&str; 5] = ["north", "south", "nixos-config", "makgora", "wisp"];
const EVENTS: &str = "safe-push/events.jsonl";
const ROTATE_BYTES: u64 = 8 << 20;
const EVENT_HORIZON: f64 = 7.0 * 86_400.0;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Event {
    pub time: f64,
    pub repo: String,
    pub lane: String,
    pub sha: String,
    pub stage: String,
    pub status: String,
    pub detail: String,
    pub seconds: f64,
    pub pid: u32,
    pub session: String,
    #[serde(skip)]
    pub tests: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StageSample {
    pub repo: String,
    pub stage: String,
    pub seconds: f64,
    pub total: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ticket {
    pub queue: String,
    pub time: f64,
    pub pid: u32,
    pub repo: String,
    pub lane: String,
    pub session: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Run {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub head_branch: String,
    pub head_sha: String,
    pub event: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub html_url: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepoFacts {
    pub name: String,
    pub slug: String,
    pub autoland: bool,
    pub runs: Vec<Run>,
    pub error: Option<String>,
    pub land_branches: Vec<String>,
}

pub fn gather(state: &Path) -> WatchContext {
    let now = super::now();
    let events = read_events(state, now as f64);
    let repos: Vec<RepoFacts> = repo_names().iter().filter_map(|name| repo_facts(name)).collect();
    let unavailable = repos.iter().filter(|repo| repo.error.is_some()).map(|repo| format!("gh:{}:", repo.name)).collect();
    WatchContext {
        now,
        state: state.to_path_buf(),
        events,
        stages: read_stages(&state.join("safe-push/stages.tsv")),
        tickets: read_tickets(&state.join("safe-push")),
        live: live_safe_pushes(),
        repos,
        settle: fs::read_to_string(state.join("north/runtime-settle.json")).ok().and_then(|text| serde_json::from_str(&text).ok()),
        unavailable,
    }
}

fn repo_names() -> Vec<String> {
    match std::env::var("NORTH_WATCH_REPOS") {
        Ok(names) if !names.trim().is_empty() => names.split_whitespace().map(str::to_owned).collect(),
        _ => DEFAULT_REPOS.iter().map(|name| (*name).to_owned()).collect(),
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn output(program: &str, directory: Option<&Path>, arguments: &[&str]) -> Result<String, String> {
    let mut command = Command::new("timeout");
    command.arg("30").arg(program).args(arguments);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let result = command.output().map_err(|error| format!("{program}: {error}"))?;
    if result.status.success() {
        return Ok(String::from_utf8_lossy(&result.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&result.stderr);
    Err(format!("{program} {}: {}", arguments.first().copied().unwrap_or_default(), stderr.trim().chars().take(200).collect::<String>()))
}

pub fn slug_of(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let mut parts = url.rsplit(['/', ':']);
    let (name, owner) = (parts.next()?, parts.next()?);
    (!name.is_empty() && !owner.is_empty()).then(|| format!("{owner}/{name}"))
}

fn repo_facts(name: &str) -> Option<RepoFacts> {
    let checkout = home().join("code").join(name).join("main");
    let slug = slug_of(&output("git", Some(&checkout), &["remote", "get-url", "origin"]).ok()?)?;
    let autoland = checkout.join(".github/workflows/autoland.yml").is_file();
    let mut facts = RepoFacts { name: name.to_owned(), slug: slug.clone(), autoland, ..RepoFacts::default() };
    match output("gh", None, &["api", &format!("repos/{slug}/actions/runs?per_page=40")]) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(body) => facts.runs = serde_json::from_value(body["workflow_runs"].clone()).unwrap_or_default(),
            Err(error) => facts.error = Some(format!("gh api answered unreadable JSON: {error}")),
        },
        Err(error) => facts.error = Some(error),
    }
    if autoland {
        match output("git", Some(&checkout), &["ls-remote", "origin", "refs/heads/claude/land-*"]) {
            Ok(text) => facts.land_branches = text.lines().filter_map(|line| line.split('\t').nth(1)).map(|branch| branch.trim_start_matches("refs/heads/").to_owned()).collect(),
            Err(error) => facts.error = facts.error.take().or(Some(error)),
        }
    }
    Some(facts)
}

pub fn parse_events(text: &str, now: f64) -> Vec<Event> {
    text.lines().filter_map(|line| serde_json::from_str::<Event>(line).ok()).filter(|event| now - event.time <= EVENT_HORIZON).collect()
}

fn read_events(state: &Path, now: f64) -> Vec<Event> {
    let path = state.join(EVENTS);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > ROTATE_BYTES) {
        let _ = fs::rename(&path, path.with_extension("jsonl.1"));
    }
    let text = [path.with_extension("jsonl.1"), path].iter().filter_map(|path| fs::read_to_string(path).ok()).collect::<Vec<_>>().join("\n");
    let mut events = parse_events(&text, now);
    for event in events.iter_mut().filter(|event| event.status == "failed" && event.stage == "land check") {
        if let Some(log) = event.detail.rsplit_once("log: ").map(|(_, log)| log.trim()) {
            event.tests = failing_tests(&fs::read_to_string(log).unwrap_or_default());
        }
    }
    events
}

pub fn failing_tests(log: &str) -> Vec<String> {
    crate::landing::red_check::failures(log).iter().map(|failure| failure.label().to_owned()).collect()
}

pub fn parse_stages(text: &str) -> Vec<StageSample> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [_, repo, _, stage, ms, total] = fields.as_slice() else { return None };
            Some(StageSample { repo: (*repo).to_owned(), stage: (*stage).to_owned(), seconds: ms.parse::<f64>().ok()? / 1000.0, total: total.parse::<f64>().ok()? / 1000.0 })
        })
        .collect()
}

fn read_stages(path: &Path) -> Vec<StageSample> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let tail: Vec<&str> = text.lines().rev().take(5000).collect();
    parse_stages(&tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

pub fn parse_ticket(queue: &str, name: &str, body: &str) -> Option<Ticket> {
    let (stamp, pid) = name.split_once('-')?;
    if stamp.is_empty() || !stamp.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let fields: Vec<&str> = body.trim_end_matches('\n').split('\t').collect();
    let root = fields.first().copied().unwrap_or_default();
    let repo = root.split("/code/").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or_default();
    Some(Ticket {
        queue: queue.to_owned(),
        time: stamp.parse::<f64>().ok()? / 1_000_000.0,
        pid: pid.parse().ok()?,
        repo: repo.to_owned(),
        lane: fields.get(1).copied().unwrap_or_default().to_owned(),
        session: fields.get(3).copied().unwrap_or_default().to_owned(),
    })
}

fn read_tickets(dir: &Path) -> Vec<Ticket> {
    let mut tickets = Vec::new();
    for queue in fs::read_dir(dir).into_iter().flatten().flatten().filter(|entry| entry.file_name().to_string_lossy().ends_with(".queue")) {
        let queue_name = queue.file_name().to_string_lossy().into_owned();
        for entry in fs::read_dir(queue.path()).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let body = fs::read_to_string(entry.path()).unwrap_or_default();
            tickets.extend(parse_ticket(&queue_name, &name, &body));
        }
    }
    tickets
}

fn live_safe_pushes() -> BTreeSet<u32> {
    let mut live = BTreeSet::new();
    for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Ok(cmdline) = fs::read(entry.path().join("cmdline")) else { continue };
        if String::from_utf8_lossy(&cmdline).contains("safe-push") {
            live.insert(pid);
        }
    }
    live
}

pub fn sessions_by_lane(events: &[Event]) -> BTreeMap<(String, String), String> {
    events.iter().filter(|event| !event.session.is_empty()).map(|event| ((event.repo.clone(), event.lane.clone()), event.session.clone())).collect()
}
