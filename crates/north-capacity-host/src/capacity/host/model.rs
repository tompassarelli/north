use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::json::{num, opt};
use super::machine::env;
use super::policy::{owner_prefix, percentile};
use super::shape::{command_shape, recorded_command};

pub const WINDOW: usize = 20;
pub const MODEL_MINIMUM_RUNS: i64 = 3;
pub const SIZING_RUNS: i64 = 5;
pub const CENSOR_STALL_USEC: f64 = 1_000_000.0;
pub const CENSOR_AT_HIGH: f64 = 0.9;
pub const CENSOR_RAISE: f64 = 1.5;
pub const ANY: &str = "*";
pub const WALL_SHARES: (f64, f64) = (0.5, 0.9);
pub const DRIFT_WALL_SHARES: (f64, f64) = (0.9, 1.0);
pub const DRIFT_MEMORY_WIDEN: f64 = 1.5;

pub const MODEL_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS capacity_model (
    key TEXT NOT NULL, host TEXT NOT NULL, cache TEXT NOT NULL, runs INTEGER NOT NULL,
    wall TEXT NOT NULL, cpu TEXT NOT NULL, memory TEXT NOT NULL, floorMiB REAL, wallCensored TEXT, PRIMARY KEY (key, host, cache))";

pub const MODEL_TABLE_COLUMNS: [(&str, &str); 1] = [("wallCensored", "TEXT")];

pub const MODEL_COLUMNS: [(&str, &str); 10] = [
    ("key", "TEXT"), ("cache", "TEXT"), ("queuedSeconds", "REAL"), ("anonPeakMiB", "REAL"), ("memoryStallUsec", "REAL"), ("censored", "INTEGER"),
    ("predictedWallSeconds", "REAL"), ("predictedWallP90Seconds", "REAL"), ("predictedMemoryMiB", "REAL"), ("predictionBasis", "TEXT"),
];

pub fn command_key(owner: &str, command: &[String]) -> String {
    format!("{} {}", owner_prefix(owner), command_shape(command))
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
    pub shape: String,
    pub host: String,
    pub cache: String,
}

impl Subject {
    pub fn of(owner: &str, command: &[String], host: String, cache: &str) -> Self {
        Self {
            command: json!(recorded_command(command)).to_string(),
            key: command_key(owner, command),
            shape: command_shape(command),
            host,
            cache: cache.to_owned(),
        }
    }

    fn levels(&self) -> [(&'static str, String, &str, &str); 4] {
        let command = format!("command:{}", self.command);
        [
            ("command-host-cache", command.clone(), &self.host, &self.cache),
            ("command-host", command, &self.host, ANY),
            ("owner-shape", format!("key:{}", self.key), &self.host, ANY),
            ("shape-host", format!("shape:{}", self.shape), &self.host, ANY),
        ]
    }
}

pub fn level_of(key: &str, cache: &str) -> &'static str {
    match key.split_once(':').map_or("", |(level, _)| level) {
        "command" if cache == ANY => "command-host",
        "command" => "command-host-cache",
        "key" => "owner-shape",
        "shape" => "shape-host",
        _ => "unknown",
    }
}

pub struct Observation<'a> {
    pub subject: &'a Subject,
    pub wall_seconds: f64,
    pub cpu_seconds: f64,
    pub anon_peak_mib: f64,
    pub censored_at_mib: Option<f64>,
    pub failed: bool,
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

    pub fn censored(samples: &[(f64, bool)], (low, high): (f64, f64)) -> Self {
        Self { p50: censored_percentile(samples, low), p90: censored_percentile(samples, high) }
    }
}

/// The Kaplan-Meier quantile of samples whose flagged values are lower bounds: the smallest finished value whose estimated share reaches `share`, else the largest bound.
pub fn censored_percentile(samples: &[(f64, bool)], share: f64) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    let mut survival = 1.0;
    let mut at_risk = sorted.len() as f64;
    for &(value, bound) in &sorted {
        if !bound {
            survival *= 1.0 - 1.0 / at_risk;
            if 1.0 - survival >= share - 1e-9 {
                return value;
            }
        }
        at_risk -= 1.0;
    }
    sorted.last().map_or(f64::NAN, |last| last.0)
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
    pub drift: bool,
    pub row: ModelRow,
}

impl Prediction {
    pub fn sizes_memory(&self) -> bool {
        let memory = self.memory_mib(1.0);
        self.row.runs >= SIZING_RUNS && memory.is_finite() && memory > 0.0
    }

    pub fn memory_mib(&self, margin: f64) -> f64 {
        let widen = if self.drift { DRIFT_MEMORY_WIDEN } else { 1.0 };
        (self.row.memory.p90 * margin * widen).max(self.row.floor_mib.unwrap_or(0.0))
    }
}

fn window<T: serde::de::DeserializeOwned>(text: &str) -> Vec<T> {
    serde_json::from_str(text).unwrap_or_default()
}

fn push<T>(values: &mut Vec<T>, value: T) {
    values.push(value);
    let excess = values.len().saturating_sub(WINDOW);
    values.drain(..excess);
}

fn usable(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

#[derive(Clone, Debug, Default)]
struct Stored {
    runs: i64,
    wall: Vec<(f64, bool)>,
    cpu: Vec<f64>,
    memory: Vec<f64>,
    floor_mib: Option<f64>,
}

impl Stored {
    fn read(row: &rusqlite::Row<'_>, first: usize) -> rusqlite::Result<Self> {
        let wall: Vec<f64> = window(&row.get::<_, String>(first + 1)?);
        let bounds: Vec<bool> = row.get::<_, Option<String>>(first + 5)?.map(|text| window(&text)).unwrap_or_default();
        let unflagged = wall.len().saturating_sub(bounds.len());
        let stale = bounds.len().saturating_sub(wall.len());
        let flags = std::iter::repeat_n(false, unflagged).chain(bounds.into_iter().skip(stale));
        Ok(Self {
            runs: row.get(first)?,
            wall: wall.into_iter().zip(flags).collect(),
            cpu: window(&row.get::<_, String>(first + 2)?),
            memory: window(&row.get::<_, String>(first + 3)?),
            floor_mib: row.get(first + 4)?,
        })
    }
}

const STORED_COLUMNS: &str = "runs, wall, cpu, memory, floorMiB, wallCensored";

fn stored(db: &Connection, key: &str, host: &str, cache: &str) -> rusqlite::Result<Option<Stored>> {
    db.query_row(&format!("SELECT {STORED_COLUMNS} FROM capacity_model WHERE key = ? AND host = ? AND cache = ?"), params![key, host, cache], |row| Stored::read(row, 0)).optional()
}

pub fn update(db: &Connection, observation: &Observation<'_>) -> rusqlite::Result<()> {
    for (_, key, host, cache) in observation.subject.levels() {
        let mut stored = stored(db, &key, host, cache)?.unwrap_or_default();
        if usable(observation.wall_seconds) {
            push(&mut stored.wall, (observation.wall_seconds, observation.failed));
        }
        if !observation.failed && usable(observation.cpu_seconds) {
            push(&mut stored.cpu, observation.cpu_seconds);
        }
        if !observation.failed && observation.censored_at_mib.is_none() && usable(observation.anon_peak_mib) {
            push(&mut stored.memory, observation.anon_peak_mib);
        }
        let raised = observation.censored_at_mib.filter(|cap| cap.is_finite()).map(|cap| cap * CENSOR_RAISE);
        let floor = match (stored.floor_mib, raised) {
            (Some(floor), Some(raised)) => Some(floor.max(raised)),
            (floor, raised) => floor.or(raised),
        };
        let wall: Vec<f64> = stored.wall.iter().map(|sample| sample.0).collect();
        let bounds: Vec<bool> = stored.wall.iter().map(|sample| sample.1).collect();
        db.execute(
            "INSERT OR REPLACE INTO capacity_model (key, host, cache, runs, wall, cpu, memory, floorMiB, wallCensored) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![key, host, cache, stored.runs + 1, json!(wall).to_string(), json!(stored.cpu).to_string(), json!(stored.memory).to_string(), floor, json!(bounds).to_string()],
        )?;
    }
    Ok(())
}

fn model_row(key: String, host: String, cache: String, stored: Stored, wall_shares: (f64, f64)) -> ModelRow {
    ModelRow { key, host, cache, runs: stored.runs, wall: Quantiles::censored(&stored.wall, wall_shares), cpu: Quantiles::of(&stored.cpu), memory: Quantiles::of(&stored.memory), floor_mib: stored.floor_mib }
}

pub fn predict(db: &Connection, subject: &Subject) -> rusqlite::Result<Option<Prediction>> {
    for (basis, key, host, cache) in subject.levels() {
        if let Some(found) = stored(db, &key, host, cache)?.filter(|found| found.runs >= MODEL_MINIMUM_RUNS) {
            let drift = super::calibration::drifting(db, &subject.key, host)?;
            let shares = if drift { DRIFT_WALL_SHARES } else { WALL_SHARES };
            return Ok(Some(Prediction { basis, drift, row: model_row(key, host.to_owned(), cache.to_owned(), found, shares) }));
        }
    }
    Ok(None)
}

pub fn rows(db: &Connection, filter: Option<&str>) -> rusqlite::Result<Vec<ModelRow>> {
    let mut statement = db.prepare(&format!("SELECT key, host, cache, {STORED_COLUMNS} FROM capacity_model WHERE ?1 IS NULL OR instr(key, ?1) > 0 ORDER BY key, host, cache"))?;
    statement
        .query_map([filter], |row| Ok(model_row(row.get(0)?, row.get(1)?, row.get(2)?, Stored::read(row, 3)?, WALL_SHARES)))?
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
            "drift": prediction.is_some_and(|prediction| prediction.drift),
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
