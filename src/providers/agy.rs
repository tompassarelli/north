use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use crate::error::NorthResult;
use crate::providers::claude::runs::landing::{tally, utc};
use crate::providers::claude::runs::row::{FLAWS, SPACE, eta, prefix};
use crate::providers::ledger_rows::{self as rows, Shared};

static PRINT_MODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"Print mode: starting \([^)]*model="([^"]+)""#).unwrap());
static CONVERSATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Created conversation ([0-9a-f-]{36})").unwrap());
static PARENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})/").unwrap());
static REQUEST_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?USER_REQUEST>").unwrap());
static FRACTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\.\d{6})\d*").unwrap());
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

fn conversations(root: &Path, cutoff: f64) -> Vec<(String, String)> {
    let mut logs: Vec<PathBuf> = fs::read_dir(root.join("log"))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    logs.sort();
    let mut models: Vec<(String, String)> = Vec::new();
    for log in logs.iter().filter(|log| log.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("cli-") && name.ends_with(".log"))) {
        let modified = fs::metadata(log).and_then(|meta| meta.modified()).ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map_or(0.0, |age| age.as_secs_f64());
        if modified < cutoff {
            continue;
        }
        let text = String::from_utf8_lossy(&fs::read(log).unwrap_or_default()).into_owned();
        let Some(slug) = PRINT_MODE.captures(&text).map(|found| found[1].to_owned()) else { continue };
        for found in CONVERSATION.captures_iter(&text) {
            match models.iter_mut().find(|(known, _)| *known == found[1]) {
                Some(entry) => entry.1 = slug.clone(),
                None => models.push((found[1].to_owned(), slug.clone())),
            }
        }
    }
    models
}

fn summaries(root: &Path) -> (HashMap<String, String>, HashMap<String, String>) {
    let (mut workspaces, mut finished) = (HashMap::new(), HashMap::new());
    let path = root.join("conversation_summaries.db");
    if !path.exists() {
        return (workspaces, finished);
    }
    let Ok(db) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return (workspaces, finished) };
    let Ok(mut statement) = db.prepare("SELECT conversation_id, workspace_uris, last_modified_time FROM conversation_summaries") else { return (workspaces, finished) };
    let found = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?)));
    for (id, uris, modified) in found.into_iter().flatten().flatten() {
        workspaces.insert(id.clone(), uris.unwrap_or_default());
        let tidy = FRACTION.replace(&modified.unwrap_or_default().replacen(' ', "T", 1), "$1").replace(' ', "");
        if let Some(at) = utc(Some(&tidy)) {
            finished.insert(id, at);
        }
    }
    (workspaces, finished)
}

pub(crate) fn rows(shared: &Shared, out: &mut impl Write) -> NorthResult<()> {
    let root = std::env::var_os("AGY_CONFIG_DIR").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".gemini/antigravity-cli"),
        PathBuf::from,
    );
    let cutoff = rows::cutoff(shared.since)?;
    let (workspaces, finished) = summaries(&root);
    for (id, slug) in conversations(&root, cutoff) {
        let path = root.join("brain").join(&id).join(".system_generated/logs/transcript_full.jsonl");
        if shared.known.contains(&id) || !path.exists() {
            continue;
        }
        let steps = crate::providers::codex::runs::records(&path);
        if let Some(row) = row(shared, &id, &slug, &steps, workspaces.get(&id).map_or("", String::as_str), finished.get(&id).map(String::as_str), cutoff) {
            writeln!(out, "{row}")?;
        }
    }
    Ok(())
}

fn count(step: &Value, name: &str) -> i64 {
    step[name].as_i64().unwrap_or(0)
}

fn samples(shared: &Shared, answers: &[&Value], model: &str) -> Vec<Value> {
    answers
        .iter()
        .enumerate()
        .filter(|(_, answer)| answer["created_at"].as_str().is_some_and(|at| !at.is_empty()))
        .map(|(n, answer)| {
            let tokens = ["input_tokens", "cache_write_tokens", "cache_read_tokens", "output_tokens"].map(|name| count(answer, name) as f64);
            json!({"request": n.to_string(), "ts": answer["created_at"], "weight": shared.prices.weight(Some(model), tokens).0})
        })
        .collect()
}

fn row(shared: &Shared, id: &str, slug: &str, steps: &[Value], workspace: &str, finished_at: Option<&str>, cutoff: f64) -> Option<Value> {
    if steps.is_empty() {
        return None;
    }
    let parent = PARENT.captures(workspace).map(|found| found[1].to_owned());
    if !shared.session.is_empty() && parent.as_deref() != Some(shared.session) {
        return None;
    }
    let mut times: Vec<String> = steps.iter().filter_map(|step| step["created_at"].as_str().filter(|at| !at.is_empty()).map(str::to_owned)).collect();
    let last = times.last()?.clone();
    times.push(finished_at.filter(|finished| *finished > last.as_str()).map_or(last, str::to_owned));
    let (first, end) = (times.first()?.as_str(), times.last()?.as_str());
    let ended_at = rows::stamp(end)?;
    if ended_at < cutoff {
        return None;
    }
    let inputs: Vec<&Value> = steps.iter().filter(|step| step["type"] == "USER_INPUT").collect();
    let answers: Vec<&Value> = steps.iter().filter(|step| step["type"] == "PLANNER_RESPONSE").collect();
    let brief = inputs.first().map_or_else(String::new, |input| REQUEST_TAG.replace_all(input["content"].as_str().unwrap_or(""), "").into_owned());
    let report = answers.last().and_then(|answer| answer["content"].as_str()).unwrap_or("").to_owned();
    let final_message = answers.last().is_some_and(|answer| steps.last().is_some_and(|step| std::ptr::eq(step, *answer)) && answer["status"] == "DONE");
    let (model, effort) = match slug.rsplit_once('-') {
        Some((model, effort)) if EFFORTS.contains(&effort) => (model, effort),
        _ => (slug, "default"),
    };
    let head = prefix(&report, 600);
    let samples = samples(shared, &answers, model);
    let unpriced: Vec<&str> = if shared.prices.by_price() && !shared.prices.lists(model) { vec![model] } else { Vec::new() };
    let mut row = json!({
        "id": id,
        "session": parent,
        "brief": rows::fields(&brief),
        "started": utc(Some(first)),
        "ended": utc(Some(end)),
        "tier": format!("{model} {effort}"),
        "account": "gemini",
        "model": model,
        "effort": effort,
        "fast": 0,
        "usage_samples": samples,
        "unpriced_models": unpriced,
        "eta_min": eta(&brief),
        "status": rows::status(final_message, ended_at),
        "actual_min": rows::minutes(rows::stamp(first)?, ended_at),
        "tokens": answers.iter().map(|answer| count(answer, "output_tokens")).sum::<i64>(),
        "peak_ctx": answers.iter().map(|answer| count(answer, "input_tokens") + count(answer, "cache_read_tokens")).max().unwrap_or(0),
        "outcome": rows::outcome(&report),
        "report": SPACE.replace_all(&head, " "),
        "flaws": FLAWS.captures(&report).map_or(Value::Null, |found| json!(found[1].parse::<i64>().unwrap_or(0))),
        "backfill": shared.recorded.contains(id),
        "staffed": utc(Some(first)),
        "review_requested": if final_message { json!(utc(Some(end))) } else { Value::Null },
    });
    row.as_object_mut()?.extend(tally(&[], shared.main));
    Some(row)
}
