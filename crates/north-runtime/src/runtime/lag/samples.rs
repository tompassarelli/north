//! The worst lag each settle read and whether its episode was paged, appended to `runtime-lag.jsonl` for the scoreboard's bar.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;

pub const FILE: &str = "runtime-lag.jsonl";
const KEPT: usize = 10_080;
const TRIM_AT_BYTES: u64 = 1 << 20;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct Sample {
    pub at: i64,
    pub seconds: f64,
    pub host: String,
    pub process: String,
    #[serde(default)]
    pub paged: bool,
}

impl Sample {
    pub(super) fn of(worst: &Value, at: i64, paged: bool) -> Option<Self> {
        Some(Self { at, seconds: worst["seconds"].as_f64()?, host: worst["host"].as_str()?.to_owned(), process: worst["process"].as_str()?.to_owned(), paged })
    }
}

pub fn read(state: &Path) -> Vec<Sample> {
    fs::read_to_string(state.join(FILE)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// The slowest lag sampled inside the window and how many samples it holds.
pub fn worst_in(samples: &[Sample], start: i64, end: i64) -> Option<(f64, usize)> {
    let inside: Vec<f64> = samples.iter().filter(|sample| sample.at >= start && sample.at < end).map(|sample| sample.seconds).collect();
    inside.iter().copied().reduce(f64::max).map(|worst| (worst, inside.len()))
}

pub(super) fn record(state: &Path, sample: &Sample) {
    let path = state.join(FILE);
    let Ok(line) = serde_json::to_string(sample) else { return };
    let written = fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| writeln!(file, "{line}"));
    if let Err(error) = written {
        eprintln!("north runtime lag: could not record {line}: {error}");
        return;
    }
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > TRIM_AT_BYTES) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let _ = fs::write(&path, lines[lines.len().saturating_sub(KEPT)..].iter().map(|line| format!("{line}\n")).collect::<String>());
    }
}
