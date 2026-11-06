//! The open priority:now count per repository, noted to a history file because GitHub keeps no past
//! count; the ready table reads it for its priority bar.
use std::fs;

use serde::{Deserialize, Serialize};

use super::ready::REPOS;
use super::state_root;

const PRIORITY: &str = "priority:now";
const PRIORITY_LOG: &str = "north/scoreboard/ready-priority.jsonl";
const NOTE_EVERY_S: i64 = 1_800;

#[derive(Debug, Deserialize, Serialize)]
pub struct Noted {
    pub at: i64,
    pub repo: String,
    pub count: usize,
}

fn priority_count(repo: &str) -> Option<usize> {
    let copy = north_github_mirror::store::Mirror::local().read(&north_github_mirror::store::key(repo, north_github_mirror::store::OPEN_ISSUES))?;
    Some(copy.body.as_array()?.iter().filter(|issue| issue["labels"]["nodes"].as_array().is_some_and(|labels| labels.iter().any(|label| label["name"] == PRIORITY))).count())
}

fn noted() -> Vec<Noted> {
    fs::read_to_string(state_root().join(PRIORITY_LOG)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Records the open priority:now counts for the history the ready table reads.
pub fn record(now: i64) {
    note(now);
}

/// Appends the open priority:now counts to the daily history, at most every half hour, since GitHub keeps no past count.
pub fn note(now: i64) -> Vec<Noted> {
    let mut history = noted();
    if history.iter().any(|entry| now - entry.at < NOTE_EVERY_S) {
        return history;
    }
    let fresh: Vec<Noted> = REPOS.iter().filter_map(|repo| Some(Noted { at: now, repo: (*repo).into(), count: priority_count(repo)? })).collect();
    let path = state_root().join(PRIORITY_LOG);
    let text: String = fresh.iter().filter_map(|entry| serde_json::to_string(entry).ok()).map(|line| line + "\n").collect();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let written = fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()));
    if written.is_ok() {
        history.extend(fresh);
    }
    history
}
