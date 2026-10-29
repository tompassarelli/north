//! Telling the Fleet parent and the proxy of a stall, a broken guard or a saturated host, and again
//! every fifteen minutes while it lasts.
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::Path;

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::host::{Host, Places, valid_name, write_atomic};
use super::stall::Verdict;

const GUARDS: [&str; 4] = ["actions", "wisp", "reuse", "pins"];
const REPEAT_SECONDS: f64 = 900.0;
const TIMER_SLACK_SECONDS: f64 = 60.0;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct Record {
    pub(crate) since: f64,
    pub(crate) sent: BTreeMap<String, f64>,
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
    pub(crate) parent: Option<String>,
    pub(crate) active: bool,
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

impl Record {
    pub(crate) fn due(&self, key: &str, now: f64) -> bool {
        self.sent.get(key).is_none_or(|at| now - at >= REPEAT_SECONDS - TIMER_SLACK_SECONDS)
    }
}

fn pending_actions(record: &Record, now: f64, repo: &str, parent: &str, key: &str, text: &str) -> Vec<Action> {
    [parent, "proxy"]
        .iter()
        .map(|node| Action { repo: repo.into(), key: key.replace("{node}", node), node: (*node).into(), channel: "fleet", text: text.into() })
        .filter(|action| record.due(&action.key, now))
        .collect()
}

pub(crate) fn alert_plan(rows: &[Value], previous: &State, now: f64, nodes: &[Node]) -> (State, Vec<Action>) {
    let mut state = previous.clone();
    let mut actions = Vec::new();
    for row in rows {
        let repo = row["repo"].as_str().unwrap_or_default();
        let stranded = super::stranded::lines(row);
        let verdict = serde_json::from_value::<Verdict>(row["stall"].clone()).ok();
        if verdict.is_none() && stranded.is_empty() {
            continue;
        }
        let guards = if verdict.is_some() { guard_reasons(row) } else { Vec::new() };
        let stalled = verdict.as_ref().filter(|verdict| verdict.stalled);
        if stalled.is_none() && guards.is_empty() && stranded.is_empty() {
            state.remove(repo);
            continue;
        }
        let parent = parent_of(nodes, repo);
        let record = state.entry(repo.to_owned()).or_insert_with(|| Record { since: now, sent: BTreeMap::new() });
        for (kind, reason) in &guards {
            let what = if *kind == "pins" { "Broken pin" } else { "Underused capacity" };
            actions.extend(pending_actions(record, now, repo, &parent, &format!("guard:{kind}:fleet:{{node}}"), &format!("{what} in {repo}: {reason}")));
        }
        if let Some(verdict) = stalled {
            let runner = verdict.runner.as_deref().map_or(String::new(), |runner| format!("; oldest landing job runner {runner}"));
            let text = format!("Landing stalled in {repo}: {}{runner}", verdict.reason);
            actions.extend(pending_actions(record, now, repo, &parent, "notice:fleet:{node}", &text));
        }
        if !stranded.is_empty() {
            let text = format!("Lanes stranded in {repo}, unlanded with no live session: {}", stranded.join("; "));
            actions.extend(pending_actions(record, now, repo, &parent, "stranded:fleet:{node}", &text));
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

pub(crate) fn stall_alert(places: &Places, host: &dyn Host, rows: &[Value], now: f64) -> Result<(), String> {
    deliver(places, host, "stalls.json", |previous, nodes| alert_plan(rows, previous, now, nodes), now)
}

/// Plans from the saved state and the Fleet sessions, then sends what is due and saves what was sent.
pub(crate) fn deliver(places: &Places, host: &dyn Host, file: &str, plan: impl FnOnce(&State, &[Node]) -> (State, Vec<Action>), now: f64) -> Result<(), String> {
    let directory = places.flow();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let lock = OpenOptions::new().create(true).append(true).open(directory.join("stalls.lock")).map_err(|error| error.to_string())?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|error| error.to_string())?;
    let path = directory.join(file);
    let nodes = fleet_nodes(host).unwrap_or_default();
    let (mut state, actions) = plan(&load(&path), &nodes);
    for action in actions {
        if !valid_name(&action.node) {
            return Err("invalid Fleet session name".into());
        }
        if host.run(&["north", "fleet", "send", &action.node, &action.text], None, None).is_none() {
            println!("{}: Fleet notification error for {}", action.repo, action.node);
            continue;
        }
        if let Some(record) = state.get_mut(&action.repo) {
            record.sent.insert(action.key, now);
        }
        save(&path, &state)?;
    }
    save(&path, &state)
}
