use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::json::{num, opt};
use super::machine::{env, read_text};
use super::policy::{owner_prefix, percentile};
use super::shape::{command_shape, recorded_command};

pub const WINDOW: usize = 20;
pub const MODEL_MINIMUM_RUNS: i64 = 3;
pub const SIZING_RUNS: i64 = 5;
pub const CENSOR_STALL_USEC: f64 = 1_000_000.0;
pub const CENSOR_AT_HIGH: f64 = 0.9;
pub const CENSOR_RAISE: f64 = 1.5;
pub const ANY: &str = "*";

pub const MODEL_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS capacity_model (
    key TEXT NOT NULL, host TEXT NOT NULL, cache TEXT NOT NULL, runs INTEGER NOT NULL,
    wall TEXT NOT NULL, cpu TEXT NOT NULL, memory TEXT NOT NULL, floorMiB REAL, PRIMARY KEY (key, host, cache))";

pub const MODEL_COLUMNS: [(&str, &str); 10] = [
    ("key", "TEXT"), ("cache", "TEXT"), ("queuedSeconds", "REAL"), ("anonPeakMiB", "REAL"), ("memoryStallUsec", "REAL"), ("censored", "INTEGER"),
    ("predictedWallSeconds", "REAL"), ("predictedWallP90Seconds", "REAL"), ("predictedMemoryMiB", "REAL"), ("predictionBasis", "TEXT"),
];

pub fn command_key(owner: &str, command: &[String]) -> String {
    format!("{} {}", owner_prefix(owner), command_shape(command))
}

pub fn host_name() -> String {
    read_text("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned()
}

fn has_fingerprints(target: &Path) -> bool {
    let children = |dir: &Path| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path()).collect::<Vec<PathBuf>>();
    let nested = children(target).into_iter().flat_map(|child| children(&child));
    children(target).into_iter().chain(nested).any(|dir| std::fs::read_dir(dir.join(".fingerprint")).is_ok_and(|mut entries| entries.next().is_some()))
}

pub fn cache_state(cwd: &Path, target_dir: Option<&str>) -> &'static str {
    let target = target_dir.filter(|dir| !dir.is_empty()).map(|dir| cwd.join(dir)).or_else(|| cwd.ancestors().find(|dir| dir.join("Cargo.lock").is_file()).map(|dir| dir.join("target")));
    if target.is_some_and(|target| has_fingerprints(&target)) { "warm" } else { "cold" }
}

pub fn current_cache_state() -> &'static str {
    let cwd = std::env::current_dir().unwrap_or_default();
    cache_state(&cwd, env("CARGO_TARGET_DIR").as_deref())
}

pub fn censored(stall_usec: Option<f64>, peak_mib: f64, memory_high_mib: Option<f64>) -> bool {
    let at_high = memory_high_mib.is_some_and(|high| high.is_finite() && high > 0.0 && peak_mib >= high * CENSOR_AT_HIGH);
    stall_usec.is_some_and(|stall| stall > CENSOR_STALL_USEC) && at_high
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subject {
    pub command: String,
    pub key: String,
    pub host: String,
    pub cache: String,
}

impl Subject {
    pub fn of(owner: &str, command: &[String], host: String, cache: &str) -> Self {
        Self {
            command: json!(recorded_command(command)).to_string(),
            key: command_key(owner, command),
            host,
            cache: cache.to_owned(),
        }
    }

    fn levels(&self) -> [(&'static str, String, &str, &str); 3] {
        let command = format!("command:{}", self.command);
        [
            ("command-host-cache", command.clone(), &self.host, &self.cache),
            ("command-host", command, &self.host, ANY),
            ("owner-shape", format!("key:{}", self.key), &self.host, ANY),
        ]
    }
}

pub fn level_of(key: &str, cache: &str) -> &'static str {
    match key.split_once(':').map_or("", |(level, _)| level) {
        "command" if cache == ANY => "command-host",
        "command" => "command-host-cache",
        "key" => "owner-shape",
        _ => "unknown",
    }
}

pub struct Observation<'a> {
    pub subject: &'a Subject,
    pub wall_seconds: f64,
    pub cpu_seconds: f64,
    pub anon_peak_mib: f64,
    pub censored_at_mib: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantiles {
    pub p50: f64,
    pub p90: f64,
}

impl Quantiles {
    pub fn of(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self { p50: f64::NAN, p90: f64::NAN };
        }
        Self { p50: percentile(values, 0.5), p90: percentile(values, 0.9) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelRow {
    pub key: String,
    pub host: String,
    pub cache: String,
    pub runs: i64,
    pub wall: Quantiles,
    pub cpu: Quantiles,
    pub memory: Quantiles,
    pub floor_mib: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prediction {
    pub basis: &'static str,
    pub row: ModelRow,
}

impl Prediction {
    pub fn sizes_memory(&self) -> bool {
        let memory = self.memory_mib(1.0);
        self.row.runs >= SIZING_RUNS && memory.is_finite() && memory > 0.0
    }

    pub fn memory_mib(&self, margin: f64) -> f64 {
        (self.row.memory.p90 * margin).max(self.row.floor_mib.unwrap_or(0.0))
    }
}

fn window(text: &str) -> Vec<f64> {
    serde_json::from_str(text).unwrap_or_default()
}

fn push(values: &mut Vec<f64>, value: f64) {
    if value.is_finite() && value >= 0.0 {
        values.push(value);
        let excess = values.len().saturating_sub(WINDOW);
        values.drain(..excess);
    }
}

type Stored = (i64, String, String, String, Option<f64>);

fn stored(db: &Connection, key: &str, host: &str, cache: &str) -> rusqlite::Result<Option<Stored>> {
    db.query_row("SELECT runs, wall, cpu, memory, floorMiB FROM capacity_model WHERE key = ? AND host = ? AND cache = ?", params![key, host, cache], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
    })
    .optional()
}

pub fn update(db: &Connection, observation: &Observation<'_>) -> rusqlite::Result<()> {
    for (_, key, host, cache) in observation.subject.levels() {
        let (runs, wall, cpu, memory, floor) = stored(db, &key, host, cache)?.unwrap_or_else(|| (0, "[]".to_owned(), "[]".to_owned(), "[]".to_owned(), None));
        let (mut wall, mut cpu, mut memory) = (window(&wall), window(&cpu), window(&memory));
        push(&mut wall, observation.wall_seconds);
        push(&mut cpu, observation.cpu_seconds);
        if observation.censored_at_mib.is_none() {
            push(&mut memory, observation.anon_peak_mib);
        }
        let raised = observation.censored_at_mib.filter(|cap| cap.is_finite()).map(|cap| cap * CENSOR_RAISE);
        let floor = match (floor, raised) {
            (Some(floor), Some(raised)) => Some(floor.max(raised)),
            (floor, raised) => floor.or(raised),
        };
        let text = |values: &[f64]| json!(values).to_string();
        db.execute(
            "INSERT OR REPLACE INTO capacity_model (key, host, cache, runs, wall, cpu, memory, floorMiB) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![key, host, cache, runs + 1, text(&wall), text(&cpu), text(&memory), floor],
        )?;
    }
    Ok(())
}

fn model_row(key: String, host: String, cache: String, stored: Stored) -> ModelRow {
    let (runs, wall, cpu, memory, floor_mib) = stored;
    ModelRow { key, host, cache, runs, wall: Quantiles::of(&window(&wall)), cpu: Quantiles::of(&window(&cpu)), memory: Quantiles::of(&window(&memory)), floor_mib }
}

pub fn predict(db: &Connection, subject: &Subject) -> rusqlite::Result<Option<Prediction>> {
    for (basis, key, host, cache) in subject.levels() {
        if let Some(found) = stored(db, &key, host, cache)?.filter(|found| found.0 >= MODEL_MINIMUM_RUNS) {
            return Ok(Some(Prediction { basis, row: model_row(key, host.to_owned(), cache.to_owned(), found) }));
        }
    }
    Ok(None)
}

pub fn rows(db: &Connection, filter: Option<&str>) -> rusqlite::Result<Vec<ModelRow>> {
    let mut statement = db.prepare("SELECT key, host, cache, runs, wall, cpu, memory, floorMiB FROM capacity_model WHERE ?1 IS NULL OR instr(key, ?1) > 0 ORDER BY key, host, cache")?;
    statement
        .query_map([filter], |row| Ok(model_row(row.get(0)?, row.get(1)?, row.get(2)?, (row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?))))?
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Forecast {
    pub subject: Subject,
    pub prediction: Option<Prediction>,
}

impl Forecast {
    pub fn json(&self, charged_memory_mib: f64) -> Value {
        let prediction = self.prediction.as_ref();
        let wall = |pick: fn(&Quantiles) -> f64| opt(prediction.map(|prediction| pick(&prediction.row.wall)).filter(|value| value.is_finite()));
        json!({
            "key": self.subject.key, "host": self.subject.host, "cache": self.subject.cache,
            "basis": prediction.map_or("unknown", |prediction| prediction.basis),
            "runs": prediction.map_or(0, |prediction| prediction.row.runs),
            "wallSeconds": wall(|quantiles| quantiles.p50),
            "wallP90Seconds": wall(|quantiles| quantiles.p90),
            "memoryMiB": opt(prediction.map(|prediction| prediction.memory_mib(1.0)).filter(|memory| memory.is_finite() && *memory > 0.0)),
            "chargedMemoryMiB": num(charged_memory_mib),
        })
    }
}

pub fn forecast(db: Option<&Connection>, subject: Subject) -> Forecast {
    let prediction = db.and_then(|db| predict(db, &subject).ok().flatten());
    Forecast { subject, prediction }
}

#[cfg(test)]
#[path = "model_tests.rs"]
pub(super) mod tests;
