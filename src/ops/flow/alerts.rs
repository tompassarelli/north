//! Telling the Fleet parent and the proxy once per stall or broken guard, and again only after the
//! repository's main moves.
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::Path;

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::host::{Host, Places, valid_name, write_atomic};
use super::stall::Verdict;

const GUARDS: [&str; 4] = ["actions", "wisp", "reuse", "pins"];

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct Record {
    pub(crate) since: f64,
    pub(crate) main_sha: String,
    pub(crate) sent: Vec<String>,
}

pub(crate) type State = BTreeMap<String, Record>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Action {
    pub(crate) repo: String,
    pub(crate) key: String,
    pub(crate) node: String,
    pub(crate) channel: &'static str,
    pub(crate) text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) id: String,
    pub(crate) session: Option<String>,
    pub(crate) parent: Option<String>,
    pub(crate) provider: String,
    pub(crate) active: bool,
    pub(crate) pid: Option<u64>,
    pub(crate) cwd: String,
}

pub(crate) fn guard_reasons(row: &Value) -> Vec<(&'static str, String)> {
    GUARDS
        .iter()
        .filter_map(|kind| Some((*kind, row[*kind]["reason"].as_str().filter(|reason| !reason.is_empty())?.to_owned())))
        .collect()
}

fn parent_of(nodes: &[Node], repo: &str) -> String {
    let fragment = format!("/code/{repo}/");
    nodes
        .iter()
        .find(|node| node.active && node.cwd.contains(&fragment))
        .map_or_else(|| format!("{repo}-parent"), |node| node.parent.clone().filter(|parent| !parent.is_empty()).unwrap_or_else(|| node.id.clone()))
}

fn pending_actions(record: &Record, repo: &str, parent: &str, key: &str, text: &str) -> Vec<Action> {
    [parent, "proxy"]
        .iter()
        .map(|node| Action { repo: repo.into(), key: key.replace("{node}", node), node: (*node).into(), channel: "fleet", text: text.into() })
        .filter(|action| !record.sent.contains(&action.key))
        .collect()
}

pub(crate) fn alert_plan(rows: &[Value], previous: &State, now: f64, nodes: &[Node]) -> (State, Vec<Action>) {
    let mut state = previous.clone();
    let mut actions = Vec::new();
    for row in rows {
        let repo = row["repo"].as_str().unwrap_or_default();
        let Ok(verdict) = serde_json::from_value::<Verdict>(row["stall"].clone()) else { continue };
        let main_sha = row["main_sha"].as_str().unwrap_or_default();
        let guards = guard_reasons(row);
        if !verdict.stalled && guards.is_empty() {
            if state.get(repo).is_some_and(|record| record.main_sha != main_sha) {
                state.remove(repo);
            }
            continue;
        }
        let parent = parent_of(nodes, repo);
        let record = state.entry(repo.to_owned()).or_insert_with(|| Record { since: now, main_sha: main_sha.into(), sent: Vec::new() });
        for (kind, reason) in &guards {
            let what = if *kind == "pins" { "Broken pin" } else { "Underused capacity" };
            actions.extend(pending_actions(record, repo, &parent, &format!("guard:{kind}:fleet:{{node}}"), &format!("{what} in {repo}: {reason}")));
        }
        if verdict.stalled {
            let runner = verdict.runner.as_deref().map_or(String::new(), |runner| format!("; oldest landing job runner {runner}"));
            let text = format!("Landing stalled in {repo}: {}{runner}", verdict.reason);
            actions.extend(pending_actions(record, repo, &parent, "notice:fleet:{node}", &text));
        }
    }
    (state, actions)
}

pub(crate) fn fleet_nodes(host: &dyn Host) -> Option<Vec<Node>> {
    let listed: Value = serde_json::from_str(&host.run(&["north", "fleet", "list", "--json"], None, None)?).ok()?;
    Some(
        listed
            .as_array()?
            .iter()
            .map(|row| Node {
                id: row["name"].as_str().unwrap_or_default().into(),
                session: row["conversation"].as_str().map(str::to_owned),
                parent: row["parent"].as_str().map(str::to_owned),
                provider: row["provider"].as_str().unwrap_or_default().into(),
                active: row["status"].as_str() != Some("exited"),
                pid: row["host_pid"].as_u64(),
                cwd: row["cwd"].as_str().unwrap_or_default().into(),
            })
            .collect(),
    )
}

fn load(path: &Path) -> Result<State, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| error.to_string()),
        Err(_) => Ok(State::new()),
    }
}

fn save(path: &Path, state: &State) -> Result<(), String> {
    write_atomic(path, &(serde_json::to_string(state).map_err(|error| error.to_string())? + "\n")).map_err(|error| error.to_string())
}

pub(crate) fn stall_alert(places: &Places, host: &dyn Host, rows: &[Value], now: f64) -> Result<(), String> {
    let directory = places.flow();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let lock = OpenOptions::new().create(true).append(true).open(directory.join("stalls.lock")).map_err(|error| error.to_string())?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|error| error.to_string())?;
    let path = directory.join("stalls.json");
    let nodes = fleet_nodes(host).ok_or("Fleet session list unavailable")?;
    let (mut state, actions) = alert_plan(rows, &load(&path)?, now, &nodes);
    for action in actions {
        if !valid_name(&action.node) {
            return Err("invalid Fleet session name".into());
        }
        if host.run(&["north", "fleet", "send", &action.node, &action.text], None, None).is_none() {
            println!("{}: Fleet notification error for {}", action.repo, action.node);
            continue;
        }
        if let Some(record) = state.get_mut(&action.repo) {
            record.sent.push(action.key);
        }
        save(&path, &state)?;
    }
    save(&path, &state)
}
