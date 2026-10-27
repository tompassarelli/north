//! The dispatch record: one JSON line per launch attempt in ~/.local/state/agents/dispatch/, which
//! holds a fresh launch until its session shows, and hands an item that keeps failing to the proxy.
use super::issues::IssueRef;
use crate::ops::pacing::placement;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

const HOLD_S: f64 = 2.0 * 3600.0;
const CLOUD_HOLD_S: f64 = 6.0 * 3600.0;
const LAPTOP_SHOW_S: f64 = 15.0 * 60.0;
const RETRY_WINDOW_S: f64 = 24.0 * 3600.0;
const RETRIES: usize = 2;
const FAILED_HOLD_S: f64 = 15.0 * 60.0;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Record {
    pub at: f64,
    pub item: String,
    pub host: String,
    pub account: String,
    pub name: String,
    #[serde(default)]
    pub cloud: bool,
    pub launched: bool,
    pub detail: String,
    /// The tier it launched on, its category and whether it ran as an arm of the Sonnet-Opus comparison.
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub arm: bool,
}

fn hold(record: &Record) -> f64 {
    if !record.launched { FAILED_HOLD_S } else if record.cloud { CLOUD_HOLD_S } else { HOLD_S }
}

pub(crate) fn directory() -> PathBuf {
    placement::pace_directory().join("dispatch")
}

pub(crate) fn read(directory: &Path) -> Vec<Record> {
    std::fs::read_to_string(directory.join("launches.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub(crate) fn append(directory: &Path, record: &Record) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join("launches.jsonl")).map_err(|error| error.to_string())?;
    writeln!(file, "{}", serde_json::to_string(record).map_err(|error| error.to_string())?).map_err(|error| error.to_string())
}

/// Items launched in the last two hours, whose sessions may not show yet, and items launched twice
/// in a day without closing, which the proxy decides about.
pub(crate) fn holds(records: &[Record], at: f64) -> BTreeMap<IssueRef, String> {
    let mut tries: BTreeMap<IssueRef, Vec<&Record>> = BTreeMap::new();
    for record in records.iter().filter(|record| at - record.at < RETRY_WINDOW_S) {
        if let Some(item) = IssueRef::parse(&record.item) {
            tries.entry(item).or_default().push(record);
        }
    }
    tries
        .into_iter()
        .filter_map(|(item, tried)| {
            let last = tried.iter().max_by(|a, b| a.at.total_cmp(&b.at))?;
            let started = tried.iter().filter(|record| record.launched).count();
            let reason = if started >= RETRIES {
                format!("north dispatch, {started} tries in 24 h without closing; the proxy decides the next one")
            } else if at - last.at < hold(last) {
                format!("north dispatch launch {} on {} {:.0} min ago", last.name, last.host, (at - last.at) / 60.0)
            } else {
                return None;
            };
            Some((item, reason))
        })
        .collect()
}

/// Sessions dispatched to `host` in the last three hours, six for the cloud lane, whose issue is
/// still open: the laptop and cloud lane counts, since no host here can list their sessions.
pub(crate) fn recent_on(records: &[Record], host: &str, open: &BTreeSet<IssueRef>, at: f64) -> i64 {
    let live = |record: &&Record| record.launched && record.host == host && at - record.at < if record.cloud { CLOUD_HOLD_S } else { LAPTOP_SHOW_S };
    records.iter().filter(live).filter(|record| IssueRef::parse(&record.item).is_some_and(|item| open.contains(&item))).count() as i64
}
