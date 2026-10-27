//! A box whose last dispatched worker ended its report with `Stop: decision` stays skipped until the
//! issue gets a comment or body edit newer than that report.
use super::issues::{Issue, IssueRef};
use super::record::Record;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*decision\b").expect("stop pattern"));

/// Each item's newest launch that started a session, which holds the report dispatch last waited on.
pub(crate) fn last_launches(records: &[Record]) -> BTreeMap<IssueRef, &Record> {
    let mut newest: BTreeMap<IssueRef, &Record> = BTreeMap::new();
    for record in records.iter().filter(|record| record.launched && !record.cloud) {
        if let Some(item) = IssueRef::parse(&record.item)
            && newest.get(&item).is_none_or(|seen| seen.at <= record.at)
        {
            newest.insert(item, record);
        }
    }
    newest
}

/// When the session's final report named `Stop: decision`, from `north fleet show NAME --json`.
pub(crate) fn stopped_at(session: &Value) -> Option<f64> {
    let report = session["entries"].as_array()?.iter().rfind(|entry| entry["kind"] == "assistant")?;
    let ended = session["turns"].as_array()?.last()?["ended_ms"].as_f64()?;
    STOP.is_match(report["text"].as_str()?).then_some(ended / 1000.0)
}

/// Whether the issue is unchanged since the report that stopped for a decision.
pub(crate) fn awaits_decision(issue: &Issue) -> bool {
    issue.stopped.is_some_and(|stopped| issue.touched <= stopped)
}
