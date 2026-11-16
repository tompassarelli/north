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
