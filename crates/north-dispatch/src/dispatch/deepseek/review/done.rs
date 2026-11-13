//! The DeepSeek sessions that reported `Done:`, found in each host's listing and its report, and the ones left alone
//! because their issue or box is no longer open.

use super::super::super::issues::{self, Issue, IssueRef, references};
use super::super::super::record::Record;
use super::super::super::{facets, fleets};
use super::super::PROVIDER;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

const QUEUED: &str = "deepseek-reviews.jsonl";
const REQUEUE_S: f64 = 30.0 * 60.0;
const RETRY_S: f64 = 60.0 * 60.0;
const LANDING_LABEL: &str = "area:landing";
const RESTING: [&str; 2] = ["idle", "parked"];

/// A DeepSeek session that reported Done and waits for its review job.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub name: String,
    pub host: String,
    pub item: IssueRef,
    pub box_text: Option<String>,
    pub category: String,
    pub landing: bool,
    /// The Done-when tests of the box, named by their `test(/NAME/)` filters.
    pub oracle: Vec<String>,
    /// When its last turn ended, which orders the oldest report first.
    pub at_ms: u64,
}

/// A DeepSeek session that reported Done whose work no review job takes, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub name: String,
    pub host: String,
    pub why: String,
    /// Whether the fleet retired it, so the review record keeps it left for good.
    pub retired: bool,
}

/// The sessions that reported Done, oldest first, and those left alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub done: Vec<Done>,
    pub left: Vec<Left>,
}

#[derive(Deserialize, Serialize)]
struct Queued {
    at: f64,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failed: Option<String>,
}

/// The sessions the review record names: queued within `REQUEUE_S`, so one Done report queues one job, or failed within
/// `RETRY_S`, so a failing job yields its turn to the sessions behind it; and queued or left ever.
#[derive(Clone, Debug, Default)]
pub struct Marked {
    pub recent: BTreeSet<String>,
    pub ever: BTreeSet<String>,
}

/// The sessions dispatch queued a review job for, as of `at`.
pub fn queued(directory: &Path, at: f64) -> Marked {
    let text = std::fs::read_to_string(directory.join(QUEUED)).unwrap_or_default();
    let marks: Vec<Queued> = text.lines().filter_map(|line| serde_json::from_str::<Queued>(line).ok()).collect();
    let within = |queued: &&Queued| at - queued.at < if queued.failed.is_some() { RETRY_S } else { REQUEUE_S };
    Marked { recent: marks.iter().filter(within).map(|queued| queued.name.clone()).collect(), ever: marks.into_iter().filter(|queued| queued.failed.is_none()).map(|queued| queued.name).collect() }
}

pub(super) fn mark(directory: &Path, name: &str, at: f64, left: Option<&str>, failed: Option<&str>) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(QUEUED)).map_err(|error| error.to_string())?;
    let line = format!("{}\n", serde_json::to_string(&Queued { at, name: name.to_owned(), left: left.map(str::to_owned), failed: failed.map(str::to_owned) }).map_err(|error| error.to_string())?);
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())
}

fn reported_done(report: &Value) -> bool {
    report["entries"].as_array().and_then(|entries| entries.iter().rev().find(|entry| entry["kind"] == "assistant")).and_then(|entry| entry["text"].as_str()).is_some_and(|text| text.trim_start().starts_with("Done:"))
}

/// Whether `session` is the provider's and waits for its review job: resting with none queued lately, or retired by its
/// host before any job named it.
fn waits(session: &Value, marked: &Marked) -> bool {
    let name = session["name"].as_str().unwrap_or_default();
    session["provider"] == PROVIDER
        && if session["past"] == true { session["ended"] == "retired" && !marked.ever.contains(name) && !marked.recent.contains(name) } else { RESTING.contains(&session["status"].as_str().unwrap_or_default()) && !marked.recent.contains(name) }
}

/// Whether `listing` holds a session of the provider that waits for its review job.
pub fn has_waiting(listing: &Value, marked: &Marked) -> bool {
    listing.as_array().into_iter().flatten().any(|session| waits(session, marked))
}

/// Why the work of a session on `issue`'s box `box_text` is left alone, or `None` while that box is open.
fn left_because(issue: Option<&Issue>, box_text: Option<&str>) -> Option<&'static str> {
    let Some(issue) = issue else { return Some("its issue is not among the open issues dispatch reads") };
    match box_text {
        Some(text) if issues::box_ticked(&issue.body, text) => Some("its box is ticked"),
        Some(text) if !issue.body.contains(text.trim()) => Some("its box is no longer in its issue"),
        None if issue.unticked() == 0 => Some("its issue has no open box"),
        _ => None,
    }
}

/// The DeepSeek sessions in each host's listing that wait for their review job and whose last report starts `Done:`,
/// oldest first, apart from those whose issue is not open or whose box is not, which are left alone.
pub fn done(lists: &[(String, Value)], reports: &[(String, Value)], records: &[Record], issues: &[Issue], marked: &Marked) -> Found {
    let mut found = Found::default();
    for (host, listing) in lists {
        let Some(report) = reports.iter().find(|(name, _)| name == host).map(|(_, report)| report) else { continue };
        for session in listing.as_array().into_iter().flatten().filter(|session| waits(session, marked)) {
            let name = session["name"].as_str().unwrap_or_default();
            if !reported_done(&report[name]) {
                continue;
            }
            let tasks = session["tasks"].as_array().map_or_else(|| session["task_ref"].as_str().unwrap_or_default().to_owned(), |tasks| tasks.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "));
            let Some(item) = references(&tasks, "").into_iter().find(|item| !item.repo.is_empty()).or_else(|| fleets::named_items(name).into_iter().next()) else { continue };
            let issue = issues.iter().find(|issue| issue.reference() == item);
            let box_text = session["box"].as_str();
            if let Some(why) = left_because(issue, box_text) {
                found.left.push(Left { name: name.to_owned(), host: host.clone(), why: format!("{item}: {why}"), retired: session["past"] == true });
                continue;
            }
            let category = records.iter().rev().find(|record| record.name == name).and_then(|record| record.category.clone()).or_else(|| issue.and_then(|issue| facets::declared_in(&issue.body, "Category")).map(str::to_owned)).unwrap_or_else(|| "unknown".into());
            let landing = issue.is_some_and(|issue| issue.labels.iter().any(|label| label == LANDING_LABEL));
            let at_ms = report[name]["turns"].as_array().and_then(|turns| turns.iter().filter_map(|turn| turn["ended_ms"].as_u64()).max()).or_else(|| session["created_ms"].as_u64()).unwrap_or_default();
            found.done.push(Done { name: name.to_owned(), host: host.clone(), item, box_text: box_text.map(str::to_owned), category, landing, oracle: issue.map(|issue| oracle_tests(&issue.body, box_text)).unwrap_or_default(), at_ms });
        }
    }
    found.done.sort_by_key(|done| done.at_ms);
    found
}

/// The test names the box's own line of `body` gives as `test(/NAME/)` filters.
fn oracle_tests(body: &str, box_text: Option<&str>) -> Vec<String> {
    let line = box_text.and_then(|text| body.lines().find(|line| line.contains(text))).unwrap_or_default();
    line.split("test(/").skip(1).filter_map(|rest| rest.split_once('/').map(|(name, _)| name.to_owned())).collect()
}
