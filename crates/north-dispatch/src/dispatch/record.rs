//! The dispatch record: one JSON line per launch attempt in ~/.local/state/agents/dispatch/, which
//! holds a fresh launch until its session shows, and hands an item that keeps failing to the proxy.
use super::issues::IssueRef;
use crate::pacing::placement;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

const HOLD_S: f64 = 2.0 * 3600.0;
const CLOUD_HOLD_S: f64 = 6.0 * 3600.0;
pub const LAPTOP_SHOW_S: f64 = 15.0 * 60.0;
const RETRY_WINDOW_S: f64 = 24.0 * 3600.0;
const RETRIES: usize = 2;
const FAILED_HOLD_S: f64 = 15.0 * 60.0;
pub const FAILURE_WINDOW_S: f64 = 10.0 * 60.0;
const FAILURES: usize = 3;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Record {
    pub at: f64,
    pub item: String,
    pub host: String,
    pub account: String,
    pub name: String,
    #[serde(default)]
    pub cloud: bool,
    pub launched: bool,
    pub detail: String,
    /// The tier it launched on and its category.
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    /// The one box of its item the launch staffs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_text: Option<String>,
    #[serde(default)]
    pub hand: bool,
    /// The effort arm it ran on, as its `Arm:` stamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_arm: Option<String>,
    /// The spin signal that climbed this launch, which makes it one row of the escalation ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    /// The higher-rung escalation this launch shadows for a lift check, so `north dash` can compare the
    /// higher rung's landed rate with the parallel lower rung's; absent on a launch that is no lift try.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lift: Option<String>,
    /// The effort it overrode its tier's with, whether drawn on an arm, tuned or escalated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_role: Option<String>,
    /// The route it ran: `small-first`, or `large-after:NAME` for the second attempt of the small-first launch NAME (north#1233).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

impl Record {
    pub fn failed(&self) -> bool {
        !self.launched
    }
}

fn hold(record: &Record) -> f64 {
    if !record.launched { FAILED_HOLD_S } else if record.cloud { CLOUD_HOLD_S } else { HOLD_S }
}

pub fn directory() -> PathBuf {
    placement::pace_directory().join("dispatch")
}

pub fn stderr_path(name: &str) -> PathBuf {
    directory().join("launch-stderr").join(format!("{name}.log"))
}

#[derive(Debug, PartialEq)]
pub struct FailurePage {
    pub cause: String,
    pub count: usize,
    pub stderr: String,
}

impl FailurePage {
    pub fn line(&self) -> String {
        format!("north fleet dispatch: {} launches failed in 10 min with the same cause: {}; first stderr: {}", self.count, self.cause, self.stderr)
    }

    /// The fleet store's condition key for this cause, so every dispatch run that still sees it repeats one condition.
    pub fn condition(&self) -> String {
        format!("launch-failure:{}", north_base::foundation::cause::key(&self.cause))
    }
}

pub fn failure_pages(records: &[Record], at: f64) -> Vec<FailurePage> {
    let mut groups: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for record in records.iter().filter(|record| !record.launched && !record.hand && at - record.at <= FAILURE_WINDOW_S && at >= record.at) {
        groups.entry(north_base::foundation::cause::key(&record.detail)).or_default().push(record);
    }
    groups
        .into_values()
        .filter(|records| records.len() >= FAILURES)
        .map(|mut records| {
            records.sort_by(|a, b| a.at.total_cmp(&b.at));
            let first = records[0];
            let stderr = if first.cloud { "none (cloud launch)".to_owned() } else { stderr_path(&first.name).display().to_string() };
            FailurePage { cause: first.detail.clone(), count: records.len(), stderr }
        })
        .collect()
}

pub fn read(directory: &Path) -> Vec<Record> {
    std::fs::read_to_string(directory.join("launches.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Appends `record` in one write, so launches recording side by side never interleave their lines.
pub fn append(directory: &Path, record: &Record) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join("launches.jsonl")).map_err(|error| error.to_string())?;
    let line = format!("{}\n", serde_json::to_string(record).map_err(|error| error.to_string())?);
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())
}

/// The started local launches of each item, or each box of an item, launched twice or more in the
/// last day, whose sessions decide whether those tries count.
pub fn contested(records: &[Record], at: f64) -> Vec<&Record> {
    let mut started: BTreeMap<(IssueRef, Option<String>), Vec<&Record>> = BTreeMap::new();
    for record in records.iter().filter(|record| record.launched && !record.cloud && !record.hand && at - record.at < RETRY_WINDOW_S) {
        if let Some(item) = IssueRef::parse(&record.item) {
            started.entry((item, record.box_text.as_deref().and_then(crate::threads::claims::box_slug))).or_default().push(record);
        }
    }
    started.into_values().filter(|tried| tried.len() >= RETRIES).flatten().collect()
}

/// The boxes launched twice in a day that the proxy decides about, as `holds` does for a whole item: for each item the slug of each such box with why.
/// A launch whose session `unspent` names did no work on the box is no try.
pub fn box_retries(records: &[Record], at: f64, unspent: &BTreeMap<String, String>) -> BoxHolds {
    let mut tries: BTreeMap<(IssueRef, String), Vec<&str>> = BTreeMap::new();
    for record in records.iter().filter(|record| record.launched && !record.hand && !unspent.contains_key(&record.name) && at - record.at < RETRY_WINDOW_S) {
        let (Some(item), Some(slug)) = (IssueRef::parse(&record.item), record.box_text.as_deref().and_then(crate::threads::claims::box_slug)) else { continue };
        tries.entry((item, slug)).or_default().push(&record.name);
    }
    let mut held = BoxHolds::new();
    for ((item, slug), names) in tries.into_iter().filter(|(_, names)| names.len() >= RETRIES) {
        held.entry(item).or_default().insert(slug, format!("north fleet dispatch, {} tries of this box in 24 h without ticking it ({}); the proxy decides the next one", names.len(), names.join(", ")));
    }
    held
}

/// Items launched in the last two hours, whose sessions may not show yet, and items tried twice in a
/// day without closing, which the proxy decides about. A launch whose session `unspent` names did
/// no work on the item is no try. A launch whose session is in `gone`, exited or forgotten, holds
/// its item no longer.
pub fn holds(records: &[Record], at: f64, unspent: &BTreeMap<String, String>, gone: &BTreeSet<String>) -> BTreeMap<IssueRef, String> {
    let mut tries: BTreeMap<IssueRef, Vec<&Record>> = BTreeMap::new();
    for record in records.iter().filter(|record| !record.hand && record.box_text.is_none() && at - record.at < RETRY_WINDOW_S) {
        if let Some(item) = IssueRef::parse(&record.item) {
            tries.entry(item).or_default().push(record);
        }
    }
    tries
        .into_iter()
        .filter_map(|(item, tried)| {
            let last = tried.iter().filter(|record| !gone.contains(&record.name)).max_by(|a, b| a.at.total_cmp(&b.at));
            let (counted, spared): (Vec<&Record>, Vec<&Record>) = tried.iter().filter(|record| record.launched).partition(|record| !unspent.contains_key(&record.name));
            let reason = if counted.len() >= RETRIES {
                let names: Vec<&str> = counted.iter().map(|record| record.name.as_str()).collect();
                let spared: Vec<String> = spared.iter().map(|record| format!("{}, which {}", record.name, unspent[&record.name])).collect();
                let spared = if spared.is_empty() { String::new() } else { format!("; not counted: {}", spared.join("; ")) };
                format!("north fleet dispatch, {} tries in 24 h without closing ({}){spared}; the proxy decides the next one", counted.len(), names.join(", "))
            } else if let Some(last) = last.filter(|last| at - last.at < hold(last)) {
                format!("north fleet dispatch launch {} on {} {:.0} min ago", last.name, last.host, (at - last.at) / 60.0)
            } else {
                return None;
            };
            Some((item, reason))
        })
        .collect()
}

/// Items whose newest launch started a session in the last two hours, six for the cloud lane: the
/// sessions dispatch holds them for, which may not show in any fleet list yet, less those in `gone`.
pub fn launching(records: &[Record], at: f64, gone: &BTreeSet<String>) -> BTreeMap<IssueRef, String> {
    let mut newest: BTreeMap<IssueRef, &Record> = BTreeMap::new();
    for record in records.iter().filter(|record| !record.hand && record.box_text.is_none() && !gone.contains(&record.name)) {
        if let Some(item) = IssueRef::parse(&record.item)
            && newest.get(&item).is_none_or(|seen| seen.at <= record.at)
        {
            newest.insert(item, record);
        }
    }
    newest.into_iter().filter(|(_, last)| last.launched && at - last.at < hold(last)).map(|(item, last)| (item, format!("north fleet dispatch launch {} on {} {:.0} min ago", last.name, last.host, (at - last.at) / 60.0))).collect()
}

/// The boxes a dispatch launch staffed within its hold, whose sessions may not show in any fleet list yet, less those in `gone`: for each item the slug of each box with the launch that holds it.
pub fn box_launching(records: &[Record], at: f64, gone: &BTreeSet<String>) -> BoxHolds {
    let mut held = BoxHolds::new();
    for record in records.iter().filter(|record| record.launched && !record.hand && !gone.contains(&record.name) && at - record.at < hold(record)) {
        let (Some(item), Some(slug)) = (IssueRef::parse(&record.item), record.box_text.as_deref().and_then(crate::threads::claims::box_slug)) else { continue };
        held.entry(item).or_default().insert(slug, format!("north fleet dispatch launch {} on {} {:.0} min ago", record.name, record.host, (at - record.at) / 60.0));
    }
    held
}

/// The boxes of each item a worker holds, by box slug, with the holder: what an issue's per-issue cap counts.
pub type BoxHolds = BTreeMap<IssueRef, BTreeMap<String, String>>;

/// Sessions dispatched to `host` in the last three hours, six for the cloud lane, whose issue is
/// still open: the laptop and cloud lane counts, since no host here can list their sessions.
pub fn recent_on(records: &[Record], host: &str, open: &BTreeSet<IssueRef>, at: f64) -> i64 {
    let live = |record: &&Record| record.launched && !record.hand && record.host == host && at - record.at < if record.cloud { CLOUD_HOLD_S } else { LAPTOP_SHOW_S };
    records.iter().filter(live).filter(|record| IssueRef::parse(&record.item).is_some_and(|item| open.contains(&item))).count() as i64
}

/// When each cloud-lane session was launched, from the dispatch record.
pub fn cloud_launch_times() -> Vec<f64> {
    read(&directory()).into_iter().filter(|record| record.cloud && record.launched).map(|record| record.at).collect()
}
