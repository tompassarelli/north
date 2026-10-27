use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Value, json};

use crate::error::NorthResult;
use crate::providers::claude::runs::landing::{Step, tally, utc};
use crate::providers::claude::runs::row::{FLAWS, SPACE, eta, prefix};
use crate::providers::ledger_rows::{self as rows, Shared};

mod usage;
mod walk;

pub(crate) use walk::records;
use walk::Walk;

struct Store {
    paths: HashMap<String, String>,
    models: HashMap<String, (Option<String>, Option<String>)>,
    candidates: Vec<String>,
}

pub(crate) fn rows(shared: &Shared, out: &mut impl Write) -> NorthResult<()> {
    let root = std::env::var_os("CODEX_CONFIG_DIR").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/north-data/codex-pooled"),
        PathBuf::from,
    );
    let cutoff = rows::cutoff(shared.since);
    let store = read_store(&root, cutoff);
    let nodes = std::cell::OnceCell::new();
    let mut parents: HashMap<String, HashMap<String, Value>> = HashMap::new();
    for path in &store.candidates {
        let mut briefs = |parent: &str| {
            parents.entry(parent.to_owned()).or_insert_with(|| spawned(store.paths.get(parent).map_or(Path::new(""), |path| Path::new(path)))).clone()
        };
        let Some(walk) = Walk::read(Path::new(path), shared, &store.models, &mut briefs) else { continue };
        if !walk.finished(shared, cutoff) {
            continue;
        }
        let role = rows::session_role(nodes.get_or_init(rows::fleet_nodes).as_ref(), &walk.id);
        if role.is_none() && !shared.recorded.contains(&walk.id) {
            continue;
        }
        if let Some(row) = row(shared, &walk, role) {
            writeln!(out, "{}", Value::Object(row))?;
        }
    }
    Ok(())
}

fn read_store(root: &Path, cutoff: f64) -> Store {
    let state = root.join("sqlite/state_5.sqlite");
    let mut store = Store { paths: HashMap::new(), models: HashMap::new(), candidates: Vec::new() };
    if state.exists() {
        let Ok(db) = Connection::open_with_flags(&state, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI) else { return store };
        let query = |sql: &str, bind: bool| -> Vec<(String, Option<String>, Option<String>)> {
            let Ok(mut statement) = db.prepare(sql) else { return Vec::new() };
            let map = |row: &rusqlite::Row| Ok((row.get::<_, Option<String>>(0)?.unwrap_or_default(), row.get::<_, Option<String>>(1).ok().flatten(), row.get::<_, Option<String>>(2).ok().flatten()));
            let rows = if bind { statement.query_map([cutoff], map) } else { statement.query_map([], map) };
            rows.map(|rows| rows.flatten().collect()).unwrap_or_default()
        };
        for (id, path, _) in query("SELECT id, rollout_path, NULL FROM threads", false) {
            if let Some(path) = path {
                store.paths.insert(id, path);
            }
        }
        for (id, model, effort) in query("SELECT id, model, reasoning_effort FROM threads", false) {
            store.models.insert(id, (model, effort));
        }
        store.candidates = query("SELECT rollout_path, NULL, NULL FROM threads WHERE updated_at >= ?", true).into_iter().map(|(path, _, _)| path).filter(|path| !path.is_empty()).collect();
        return store;
    }
    for path in sessions(&root.join("sessions")) {
        let fresh = fs::metadata(&path).and_then(|meta| meta.modified()).ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).is_some_and(|age| age.as_secs_f64() >= cutoff);
        if !fresh {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        let meta = text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).find(|record| record["type"] == "session_meta");
        if let Some(id) = meta.as_ref().and_then(|meta| meta["payload"]["id"].as_str()) {
            store.paths.insert(id.to_owned(), path.to_string_lossy().into_owned());
        }
        store.candidates.push(path.to_string_lossy().into_owned());
    }
    store
}

fn sessions(root: &Path) -> Vec<PathBuf> {
    let list = |directory: &Path| -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(directory).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
        found.sort();
        found
    };
    let mut found = Vec::new();
    for year in list(root) {
        for month in list(&year) {
            for day in list(&month) {
                found.extend(list(&day).into_iter().filter(|path| {
                    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
                }));
            }
        }
    }
    found.sort();
    found
}

fn spawned(path: &Path) -> HashMap<String, Value> {
    let (mut calls, mut named): (HashMap<String, Value>, HashMap<String, Value>) = (HashMap::new(), HashMap::new());
    for record in walk::records(path) {
        let payload = &record["payload"];
        let kind = payload["type"].as_str().unwrap_or("");
        if kind == "function_call" && payload["name"].as_str().is_some_and(|name| name.ends_with("spawn_agent")) {
            let arguments = payload["arguments"].as_str().and_then(|text| serde_json::from_str::<Value>(text).ok());
            if let (Some(Value::Object(mut arguments)), Some(call)) = (arguments, payload["call_id"].as_str()) {
                arguments.insert("staffed".into(), record["timestamp"].clone());
                calls.insert(call.to_owned(), Value::Object(arguments));
            }
        }
        if kind == "function_call_output"
            && let Some(call) = payload["call_id"].as_str().and_then(|call| calls.get(call))
            && let Some(name) = payload["output"].as_str().and_then(|text| serde_json::from_str::<Value>(text).ok()).and_then(|output| output["task_name"].as_str().map(str::to_owned))
        {
            named.insert(name, call.clone());
        }
    }
    named
}

fn digits(name: &str) -> HashSet<String> {
    let mut runs = HashSet::new();
    let mut current = String::new();
    for character in name.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_digit() {
            current.push(character);
        } else {
            if (2..=5).contains(&current.len()) {
                runs.insert(current.clone());
            }
            current.clear();
        }
    }
    runs
}

fn row(shared: &Shared, walk: &Walk, role: Option<&'static str>) -> Option<Map<String, Value>> {
    let head = prefix(&walk.report, 400);
    let steps: Vec<Step> = walk.steps.iter().map(|(command, code, output, finished)| Step { command: Some(command.clone()), code: *code, output: output.clone(), finished: finished.clone() }).collect();
    let mut counts = tally(&steps, shared.main);
    let numbers = digits(walk.name());
    if walk.repos.len() == 1 && numbers.len() == 1 {
        let reference = format!("{}#{}", walk.repos.iter().next()?, numbers.iter().next()?);
        counts["refs"].as_array_mut()?.push(json!(reference));
    }
    let started = walk.started.as_deref()?;
    let ended = walk.ended.as_deref()?;
    let (started_at, ended_at) = (rows::stamp(started)?, rows::stamp(ended)?);
    let mut row = Map::new();
    row.insert("id".into(), json!(walk.id));
    row.insert("session".into(), json!(walk.parent));
    row.insert("brief".into(), json!(rows::fields(&walk.brief)));
    row.insert("started".into(), json!(started));
    row.insert("ended".into(), json!(ended));
    row.insert("tier".into(), json!(format!("{} {}", walk.model.as_deref()?, walk.effort.as_deref()?)));
    row.insert("eta_min".into(), eta(&walk.brief));
    row.insert("item".into(), json!(rows::declared_item(&walk.brief)));
    row.insert("category".into(), json!(rows::declared_category(&walk.brief)));
    row.insert("status".into(), json!(rows::status(walk.final_message, ended_at)));
    row.insert("actual_min".into(), json!(rows::minutes(started_at, ended_at)));
    row.insert("tokens".into(), json!(walk.tokens));
    row.insert("role".into(), json!(role));
    row.insert("telemetry_only".into(), json!(role.is_none()));
    row.insert("outcome".into(), json!(rows::outcome(&walk.report)));
    row.insert("note".into(), Value::Null);
    row.insert("report".into(), json!(prefix(&SPACE.replace_all(&head, " "), 160)));
    row.insert("flaws".into(), FLAWS.captures(&walk.report).map_or(Value::Null, |found| json!(found[1].parse::<i64>().unwrap_or(0))));
    row.insert("backfill".into(), json!(shared.recorded.contains(&walk.id)));
    row.insert("staffed".into(), json!(utc(walk.staffed.as_deref())));
    row.insert("review_requested".into(), json!(utc(walk.review.as_deref())));
    row.extend(counts);
    row.extend(usage::context_metrics(&walk.records));
    row.extend(usage::run_usage(&walk.records, shared.prices));
    Some(row)
}
