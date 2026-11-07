//! The open `priority:now` counts the ready scoreboard notes every half hour, since GitHub keeps no past count: the
//! record of whether ready work waited, read by the idle-account bar and the usage readings.
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const PATH: &str = "north/scoreboard/ready-priority.jsonl";

#[derive(Debug, Deserialize, Serialize)]
pub struct Noted {
    pub at: i64,
    pub repo: String,
    pub count: usize,
}

/// Every count noted under the state root `state`, oldest first.
pub fn read(state: &Path) -> Vec<Noted> {
    fs::read_to_string(state.join(PATH)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Whether some count noted from `start` up to `end` found priority work open; none when nothing was noted then.
pub fn waited(history: &[Noted], start: i64, end: i64) -> Option<bool> {
    let held: Vec<&Noted> = history.iter().filter(|entry| (start..end).contains(&entry.at)).collect();
    (!held.is_empty()).then(|| held.iter().any(|entry| entry.count > 0))
}
