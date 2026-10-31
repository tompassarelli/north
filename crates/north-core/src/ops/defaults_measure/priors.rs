use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::Context;
use super::sweeps;
use crate::threads::calibration::quantile;
use crate::foundation::clock::Timestamp;

type Count = (usize, Option<f64>);

pub(super) fn priors(db: &Connection, context: &Context) -> Result<Vec<String>, String> {
    let mut counted: BTreeMap<&str, Count> = BTreeMap::new();
    let mut lines = Vec::new();
    for entry in context.registry.entries().iter().filter(|entry| entry.source.starts_with("prior: ")) {
        let settles = entry.settles.as_ref().ok_or_else(|| format!("{}: a prior needs settles_when", entry.name))?;
        let count = settles.count.as_str();
        let (n, value) = match counted.get(count) {
            Some(known) => *known,
            None => {
                let measured = counter(db, context, count)?;
                counted.insert(count, measured);
                measured
            }
        };
        lines.push(if n as i64 >= settles.need {
            format!("SETTLED {} n={n}{}", entry.name, value.map(|value| format!(" value={}", general(value))).unwrap_or_default())
        } else {
            format!("WAIT {} n={n}/{}", entry.name, settles.need)
        });
    }
    Ok(lines)
}

fn counter(db: &Connection, context: &Context, name: &str) -> Result<Count, String> {
    let sql = |error: rusqlite::Error| format!("{name}: {error}");
    match name {
        "count_worker_runs" => Ok((sweeps::first_run_count(db).map_err(sql)?, None)),
        "count_capacity_episodes" => {
            let episodes = context.capacity_episodes(60.0);
            Ok((episodes.closed, episodes.p90.filter(|_| episodes.closed > 0).map(f64::ceil)))
        }
        "count_astra_runs" => astra_runs(db).map_err(sql),
        "count_goal_targets" => {
            let targets: BTreeSet<String> = jsonl_rows(&context.state_dir().join("goal-scores.jsonl"))
                .into_iter()
                .filter(|row| matches!(row["on-date"].as_str(), Some("pass" | "fail")))
                .map(|row| format!("{}\u{0}{}", row["project"], row["target"]))
                .collect();
            Ok((targets.len(), None))
        }
        "count_explored_tier_runs" => Ok((sweeps::least_explored_tier_runs(db, context.registry.number("explore_runs") as usize).map_err(sql)?, None)),
        "count_successor_outcomes" => successor_outcomes(db).map_err(sql),
        "count_capacity_denials" => {
            let rows = jsonl_rows(&context.state_dir().join("spawn-gate-denials.jsonl"));
            Ok((rows.iter().filter(|row| row["gate"] == "capacity" && truthy(&row["at"])).count(), None))
        }
        "count_meter_test_haiku_runs" => Ok(meter_test_haiku_runs(&context.state_dir().join("replay"))),
        other => Err(format!("settles_when count {other:?} is not a defaults-measure counter")),
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

fn stamp(text: &str) -> f64 {
    Timestamp::parse_python_datetime(text).map_or(f64::NAN, Timestamp::unix_seconds_f64)
}

fn astra_runs(db: &Connection) -> rusqlite::Result<Count> {
    let mut select = db.prepare(
        "SELECT ts, used_pct, resets_at FROM usage WHERE provider = 'codex' AND kind = 'window' AND name = 'week' AND used_pct IS NOT NULL ORDER BY ts",
    )?;
    let week: Vec<(f64, f64, Option<String>)> =
        select.query_map([], |row| Ok((stamp(&row.get::<_, String>(0)?), row.get(1)?, row.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut select = db.prepare("SELECT started, ended FROM runs WHERE role = 'worker' AND tier = 'gpt-6-astra medium' AND started IS NOT NULL")?;
    let runs: Vec<(String, String)> = select.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut rises = Vec::new();
    for (started, ended) in runs {
        let (start, end) = (stamp(&started), stamp(&ended));
        let before = week.iter().rfind(|(at, _, _)| *at <= start);
        let after = week.iter().find(|(at, _, _)| *at >= end);
        if let (Some(before), Some(after)) = (before, after)
            && before.2 == after.2
        {
            rises.push((after.1 - before.1).max(0.0));
        }
    }
    Ok((rises.len(), quantile(&rises, 0.5)))
}

fn successor_outcomes(db: &Connection) -> rusqlite::Result<Count> {
    let now = Timestamp::now().iso();
    let mut select = db.prepare("SELECT model, max(at) AS at FROM model_events WHERE kind IN ('expected', 'market') AND at IS NOT NULL GROUP BY model")?;
    let due: BTreeSet<Option<String>> = select
        .query_map([], |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|(_, at)| *at < now)
        .map(|(model, _)| model)
        .collect();
    Ok((due.len(), None))
}

fn meter_test_haiku_runs(replay: &Path) -> Count {
    let mut paths = Vec::new();
    jsonl_files(replay, &mut paths);
    paths.sort_by_key(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok());
    let files: Vec<Vec<Value>> = paths
        .iter()
        .map(|path| {
            jsonl_rows(path)
                .into_iter()
                .filter(|row| {
                    let model = match &row["model"] {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    model.contains("haiku") && !row["scenario"].is_null() && !row["five_hour_pct"].is_null() && !row["seven_day_pct"].is_null()
                })
                .collect()
        })
        .filter(|rows: &Vec<Value>| !rows.is_empty())
        .collect();
    let pcts: Vec<f64> = files.last().map(|rows| rows.iter().filter_map(|row| row["five_hour_pct"].as_f64()).collect()).unwrap_or_default();
    let spread = pcts.iter().copied().reduce(f64::max).zip(pcts.iter().copied().reduce(f64::min)).map(|(high, low)| high - low);
    (files.iter().map(Vec::len).sum(), spread)
}

fn jsonl_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            jsonl_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "jsonl") {
            found.push(path);
        }
    }
}

fn jsonl_rows(path: &Path) -> Vec<Value> {
    let Ok(bytes) = fs::read(path) else { return Vec::new() };
    String::from_utf8_lossy(&bytes).lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(Value::is_object).collect()
}

pub(super) fn general(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return format!("{value}");
    }
    let exponent = value.abs().log10().floor() as i32;
    if (-4..6).contains(&exponent) {
        let decimals = (5 - exponent).max(0) as usize;
        trim(format!("{value:.decimals$}"))
    } else {
        let scientific = format!("{value:.5e}");
        let (mantissa, power) = scientific.split_once('e').unwrap_or((&scientific, "0"));
        let power: i32 = power.parse().unwrap_or_default();
        format!("{}e{}{:02}", trim(mantissa.to_owned()), if power < 0 { '-' } else { '+' }, power.abs())
    }
}

fn trim(text: String) -> String {
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
}
