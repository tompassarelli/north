use super::layout::{self, Layout};
use crate::platform::process;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const JOURNAL: &str = "runtime-live.jsonl";
const SEEN: &str = "runtime-live-seen.json";
const ACTIVE_S: i64 = 86_400;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    pub sha: String,
    pub host: String,
    pub landed_at: i64,
    pub live_at: i64,
}

pub fn journal(state: &Path) -> PathBuf {
    state.join(JOURNAL)
}

pub(super) fn record(layout: &Layout, checkout: &Path, hosts: &[(String, String, u64)]) {
    let path = layout.state.join(SEEN);
    let mut seen: BTreeMap<String, String> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let mut rows = Vec::new();
    for (host, runtime, swapped_at) in hosts {
        let previous = seen.insert(host.clone(), runtime.clone());
        let Some(previous) = previous.filter(|previous| previous != runtime) else { continue };
        rows.extend(landed(checkout, &previous, runtime).into_iter().map(|(sha, landed_at)| Live { sha, host: host.clone(), landed_at, live_at: (*swapped_at as i64).max(landed_at) }));
    }
    let _ = fs::write(&path, serde_json::to_string(&seen).unwrap_or_default());
    let text: String = rows.iter().filter_map(|row| serde_json::to_string(row).ok()).map(|line| line + "\n").collect();
    if !text.is_empty() {
        let _ = fs::OpenOptions::new().create(true).append(true).open(journal(&layout.state)).and_then(|mut file| file.write_all(text.as_bytes()));
    }
}

fn landed(checkout: &Path, previous: &str, runtime: &str) -> Vec<(String, i64)> {
    let range = format!("{previous}..{runtime}");
    let Ok(output) = process::run("git", Some(checkout), &["rev-list", "--first-parent", "--format=%H %ct", "--no-commit-header", &range]) else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().filter_map(|line| line.split_once(' ')).filter(|(sha, _)| layout::is_sha(sha)).filter_map(|(sha, at)| Some((sha.to_owned(), at.trim().parse().ok()?))).collect()
}

pub fn everywhere(rows: &[Live]) -> Vec<(i64, f64)> {
    let newest = rows.iter().map(|row| row.live_at).max().unwrap_or(0);
    let mut last: HashMap<&str, i64> = HashMap::new();
    for row in rows {
        let at = last.entry(row.host.as_str()).or_insert(row.live_at);
        *at = (*at).max(row.live_at);
    }
    let hosts = last.values().filter(|at| **at >= newest - ACTIVE_S).count();
    let mut landings: HashMap<&str, (i64, i64, usize)> = HashMap::new();
    for row in rows {
        let entry = landings.entry(row.sha.as_str()).or_insert((row.landed_at, row.live_at, 0));
        entry.1 = entry.1.max(row.live_at);
        entry.2 += 1;
    }
    landings.into_values().filter(|(_, _, count)| *count >= hosts).map(|(landed_at, live_at, _)| (landed_at, (live_at - landed_at) as f64)).collect()
}

pub fn read(state: &Path) -> Vec<Live> {
    fs::read_to_string(journal(state)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}
