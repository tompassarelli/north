use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

use super::row::Row;
use crate::threads::calibration::quantile;
use crate::foundation::clock::Timestamp;

const ERROR_INFO: &str = "server_overloaded";
const MESSAGE: &str = "Selected model is at capacity";

pub(super) struct Turn {
    at: f64,
    model: Option<String>,
    outcome: &'static str,
}

pub(super) fn codex_root() -> PathBuf {
    std::env::var_os("CODEX_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| Path::new(&std::env::var("HOME").unwrap_or_default()).join("code/north-data/codex-pooled"), PathBuf::from)
}

pub(super) fn turns(root: &Path, since: f64) -> Vec<Turn> {
    let mut out: Vec<Turn> = rollouts(root, since).iter().flat_map(|path| rollout_turns(path)).filter(|turn| turn.at >= since).collect();
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    out
}

fn rollouts(root: &Path, since: f64) -> Vec<PathBuf> {
    let state = root.join("sqlite/state_5.sqlite");
    if state.exists() {
        let paths = || -> rusqlite::Result<Vec<PathBuf>> {
            let db = super::training::open_read_only(&state)?;
            let mut select = db.prepare("SELECT rollout_path FROM threads WHERE updated_at >= ?")?;
            select.query_map([since as i64], |row| row.get::<_, String>(0).map(PathBuf::from))?.collect()
        };
        return paths().unwrap_or_default();
    }
    let mut found = Vec::new();
    walk(&root.join("sessions"), since, &mut found);
    found
}

fn walk(dir: &Path, since: f64, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(&path, since, found);
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let modified = meta.modified().ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map_or(0.0, |at| at.as_secs_f64());
        if name.starts_with("rollout-") && name.ends_with(".jsonl") && modified >= since {
            found.push(path);
        }
    }
}

fn rollout_turns(path: &Path) -> Vec<Turn> {
    let Ok(text) = fs::read(path) else { return Vec::new() };
    let mut model = None;
    let mut out = Vec::new();
    for record in String::from_utf8_lossy(&text).lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        let payload = &record["payload"];
        match record["type"].as_str() {
            Some("turn_context") => model = payload["model"].as_str().filter(|name| !name.is_empty()).map(str::to_owned).or(model),
            Some("event_msg") if payload["type"] == "task_complete" => {
                let error = &payload["error"];
                let capacity = error["codex_error_info"] == ERROR_INFO || error["message"].as_str().unwrap_or_default().contains(MESSAGE);
                let failed = !error.is_null() && error != &Value::Bool(false) && error != "" && error != &Value::Object(Default::default());
                let at = record["timestamp"].as_str().and_then(Timestamp::parse_python_datetime);
                if let Some(at) = at {
                    out.push(Turn { at: at.unix_seconds_f64(), model: model.clone(), outcome: if capacity { "capacity" } else if failed { "error" } else { "ok" } });
                }
            }
            _ => {}
        }
    }
    out
}

pub(super) struct Episodes {
    pub(super) rows: Vec<Row>,
    pub(super) closed: usize,
    pub(super) p90: Option<f64>,
}

pub(super) fn capacity_episodes(turns: &[Turn]) -> Episodes {
    let mut open: Vec<(Option<&str>, f64)> = Vec::new();
    let mut spans = Vec::new();
    for turn in turns {
        let model = turn.model.as_deref();
        let held = open.iter().position(|(name, _)| *name == model);
        match (turn.outcome, held) {
            ("capacity", None) => open.push((model, turn.at)),
            ("ok", Some(index)) => spans.push((turn.at - open.remove(index).1) / 60.0),
            _ => {}
        }
    }
    let p90 = quantile(&spans, 0.9);
    let mut rows = vec![
        Row::new()
            .with("capacity_turns", turns.iter().filter(|turn| turn.outcome == "capacity").count())
            .with("turns", turns.len())
            .with("episodes", spans.len())
            .with("unclosed", open.len())
            .with("p50", quantile(&spans, 0.5))
            .with("p90", p90)
            .with("max", spans.iter().copied().reduce(f64::max)),
    ];
    for longer in [5.0, 10.0, 15.0, 30.0, 60.0] {
        rows.push(Row::new().with("value", longer as i64).with("episodes_longer", spans.iter().filter(|span| **span > longer).count()));
    }
    Episodes { rows, closed: spans.len(), p90 }
}
