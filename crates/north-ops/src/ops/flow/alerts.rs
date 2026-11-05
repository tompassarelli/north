//! Telling the Fleet parent and the proxy of a stall, a stranded or refused lane, a broken guard or a
//! saturated host. Each alert goes out once per episode, keyed on its kind and the facts that make
//! it actionable, after two consecutive runs see it, and again only after a run stops seeing it.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::path::Path;

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::host::{Host, Places, valid_name, write_atomic};
use super::stall::Verdict;
use crate::platform::rolling_issue::Breaches;

const GUARDS: [&str; 4] = ["actions", "wisp", "reuse", "pins"];
const EPISODES: [&str; 4] = ["guard", "notice", "stranded", "refused"];
pub const SUPPLY: &str = "supply";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Record {
    pub since: f64,
    pub sent: BTreeMap<String, f64>,
    #[serde(default)]
    pub breaches: Breaches,
}

pub type State = BTreeMap<String, Record>;

#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub repo: String,
    pub key: String,
    pub node: String,
    pub channel: &'static str,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
    pub active: bool,
    pub cwd: String,
}

pub fn guard_reasons(row: &Value) -> Vec<(&'static str, String)> {
    GUARDS
        .iter()
        .filter_map(|kind| Some((*kind, row[*kind]["reason"].as_str().filter(|reason| !reason.is_empty())?.to_owned())))
        .collect()
}

fn parent_of(nodes: &[Node], repo: &str) -> String {
    let fragment = format!("/code/{}/", crate::threads::catalog::checkout(repo));
    nodes
        .iter()
        .find(|node| node.active && node.cwd.contains(&fragment))
        .map_or_else(|| format!("{repo}-parent"), |node| node.parent.clone().filter(|parent| !parent.is_empty()).unwrap_or_else(|| node.id.clone()))
}

fn candidates(repo: &str, parent: &str, key: &str, text: &str) -> Vec<Action> {
    [parent, "proxy"]
        .iter()
        .map(|node| Action { repo: repo.into(), key: key.replace("{node}", node), node: (*node).into(), channel: "fleet", text: text.into() })
        .collect()
}

fn digest(parts: &[String]) -> String {
    let mut parts = parts.to_vec();
    parts.sort_unstable();
    let hash = Sha256::digest(parts.join("\n").as_bytes());
    hash.iter().take(6).map(|byte| format!("{byte:02x}")).collect()
}

/// The alert's actions for the nodes that have not heard this episode; every key of the episode joins `seen`.
fn episode_actions(record: &Record, seen: &mut BTreeSet<String>, repo: &str, parent: &str, kind: &str, identity: &[String], text: &str) -> Vec<Action> {
    let all = candidates(repo, parent, &format!("{kind}:{}:fleet:{{node}}", digest(identity)), text);
    seen.extend(all.iter().map(|action| action.key.clone()));
    all.into_iter().filter(|action| !record.sent.contains_key(&action.key)).collect()
}

fn episode_key(key: &str) -> bool {
    EPISODES.iter().any(|kind| key.strip_prefix(kind).is_some_and(|rest| rest.starts_with(':')))
}

fn stall_identity(verdict: &Verdict) -> Vec<String> {
    let mut identity: Vec<String> = verdict.overdue_jobs.iter().map(|late| format!("job {} {} {}", late.job.workflow, late.job.run_id, late.job.name)).collect();
    if verdict.reason.starts_with("oldest pending lane") {
        identity.push("pending".into());
    }
    for (prefix, name) in [("lander tickets queued", "lander-idle"), ("lander candidate check", "lander-check")] {
        if verdict.reason.split("; ").any(|part| part.starts_with(prefix)) {
            identity.push(name.into());
        }
    }
    identity
}

pub fn alert_plan(rows: &[Value], previous: &State, now: f64, nodes: &[Node]) -> (State, Vec<Action>) {
    let mut state = previous.clone();
    let mut actions = Vec::new();
    for row in rows {
        let repo = row["repo"].as_str().unwrap_or_default();
        let stranded = super::stranded::lines(row);
        let refused = super::refused::lines(row, nodes);
        let verdict = serde_json::from_value::<Verdict>(row["stall"].clone()).ok();
        if verdict.is_none() && stranded.is_empty() && refused.is_empty() {
            continue;
        }
        let guards = if verdict.is_some() { guard_reasons(row) } else { Vec::new() };
        let stalled = verdict.as_ref().filter(|verdict| verdict.stalled);
        if stalled.is_none() && guards.is_empty() && stranded.is_empty() && refused.is_empty() {
            state.remove(repo);
            continue;
        }
        let parent = parent_of(nodes, repo);
        let record = state.entry(repo.to_owned()).or_insert_with(|| Record { since: now, ..Record::default() });
        let mut seen = BTreeSet::new();
        let mut planned = Vec::new();
        for (kind, reason) in &guards {
            let what = if *kind == "pins" { "Broken pin" } else { "Underused capacity" };
            planned.extend(episode_actions(record, &mut seen, repo, &parent, &format!("guard:{kind}"), &[(*kind).to_owned()], &format!("{what} in {repo}: {reason}")));
        }
        if let Some(verdict) = stalled {
            let runner = verdict.runner.as_deref().map_or(String::new(), |runner| format!("; oldest landing job runner {runner}"));
            let text = format!("Landing stalled in {repo}: {}{runner}", verdict.reason);
            planned.extend(episode_actions(record, &mut seen, repo, &parent, "notice", &stall_identity(verdict), &text));
        }
        if !stranded.is_empty() {
            let text = format!("Lanes stranded in {repo}, unlanded with no live session: {}", stranded.join("; "));
            planned.extend(episode_actions(record, &mut seen, repo, &parent, "stranded", &super::stranded::identities(row), &text));
        }
        if !refused.is_empty() {
            let text = format!("Land branches Autoland refused in {repo}, with no live session to fix them: {}", refused.join("; "));
            planned.extend(episode_actions(record, &mut seen, repo, &parent, "refused", &super::refused::identities(row, nodes), &text));
        }
        record.sent.retain(|key, _| !episode_key(key) || seen.contains(key));
        let confirmed = record.breaches.tick(&seen);
        actions.extend(planned.into_iter().filter(|action| confirmed.contains(&action.key)));
    }
    (state, actions)
}

pub fn fleet_nodes(host: &dyn Host) -> Option<Vec<Node>> {
    let listed: Value = serde_json::from_str(&host.run(&["north", "fleet", "list", "--json"], None, None)?).ok()?;
    Some(
        listed
            .as_array()?
            .iter()
            .map(|row| Node {
                id: row["name"].as_str().unwrap_or_default().into(),
                parent: row["parent"].as_str().map(str::to_owned),
                active: row["status"].as_str() != Some("exited"),
                cwd: row["cwd"].as_str().unwrap_or_default().into(),
            })
            .collect(),
    )
}

fn load(path: &Path) -> State {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save(path: &Path, state: &State) -> Result<(), String> {
    write_atomic(path, &(serde_json::to_string(state).map_err(|error| error.to_string())? + "\n")).map_err(|error| error.to_string())
}

pub fn stall_alert(places: &Places, host: &dyn Host, rows: &[Value], now: f64) -> Result<(), String> {
    deliver(places, host, "stalls.json", |previous, nodes| alert_plan(rows, previous, now, nodes), now)
}

/// Plans from the saved state and the Fleet sessions, then sends what is due and saves what was sent.
pub fn deliver(places: &Places, host: &dyn Host, file: &str, plan: impl FnOnce(&State, &[Node]) -> (State, Vec<Action>), now: f64) -> Result<(), String> {
    let directory = places.flow();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let lock = OpenOptions::new().create(true).append(true).open(directory.join("stalls.lock")).map_err(|error| error.to_string())?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|error| error.to_string())?;
    let path = directory.join(file);
    let nodes = fleet_nodes(host).unwrap_or_default();
    let (mut state, actions) = plan(&load(&path), &nodes);
    for action in actions {
        let sent = if action.channel == SUPPLY {
            host.run(&["supply-alert", "normal", &action.text, "Tier routing"], None, None).is_some()
        } else if valid_name(&action.node) {
            host.run(&["north", "fleet", "send", "--from", "service:north-ops-flow", &action.node, &action.text], None, None).is_some()
        } else {
            return Err("invalid Fleet session name".into());
        };
        if !sent {
            println!("{}: notification error for {}", action.repo, if action.channel == SUPPLY { "supply-alert" } else { &action.node });
            continue;
        }
        if let Some(record) = state.get_mut(&action.repo) {
            record.sent.insert(action.key, now);
        }
        save(&path, &state)?;
    }
    save(&path, &state)
}
