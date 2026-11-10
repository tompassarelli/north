//! Per-stage, per-host landing-to-live rows for artifacts other than the runtime pin, written by the
//! hosts that build or activate them and read here for the firn closure bar (docs/rationale/propagation.md).
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const FILE: &str = "propagation.jsonl";
pub const FIRN_CLOSURE: &str = "firn closure";
pub const TOTAL: &str = "total";

#[derive(Debug, PartialEq, Deserialize)]
pub struct Row {
    pub artifact: String,
    pub host: String,
    pub stage: String,
    pub landed: i64,
    pub seconds: f64,
}

pub fn path(records: &Path) -> PathBuf {
    records.join("north").join(FILE)
}

pub fn read(records: &Path) -> Vec<Row> {
    fs::read_to_string(path(records)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Whole landing-to-live seconds of each landing inside the window, from every host's total row.
pub fn totals(rows: &[Row], artifact: &str, start: i64, end: i64) -> Vec<f64> {
    rows.iter().filter(|row| row.artifact == artifact && row.stage == TOTAL && row.landed >= start && row.landed < end).map(|row| row.seconds).collect()
}
