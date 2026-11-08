use std::collections::BTreeMap;
use std::fmt::Write;

use toml_edit::DocumentMut;

use crate::alarms::Alarm;
use crate::foundation::clock::Timestamp;

pub const LABELS: &str = include_str!("alarm_labels.toml");
pub const SOURCES: &[&str] = &["north-health", "north-land-watch", "ops-flow", "promise-checker"];
const OVER_PERCENT: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Label {
    pub source: String,
    pub at: i64,
    pub acted: bool,
    pub narrowed: Option<String>,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Source {
    pub alerts: usize,
    pub labelled: usize,
    pub falses: usize,
    pub narrowed: Option<String>,
}

pub fn labels(text: &str) -> Result<Vec<Label>, String> {
    let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| format!("alarm labels: {error}"))?;
    let tables = document.get("alarm").and_then(|item| item.as_array_of_tables()).ok_or("alarm labels: no [[alarm]] table")?;
    tables
        .iter()
        .map(|table| {
            let field = |name: &str| table.get(name).and_then(|item| item.as_str());
            let source = field("source").filter(|source| SOURCES.contains(source)).ok_or("alarm labels: an alarm names no known source")?;
            let at = field("at").and_then(|at| Timestamp::parse(at).ok()).ok_or("alarm labels: an alarm has no readable at")?;
            let acted = match field("label") {
                Some("acted") => true,
                Some("false") => false,
                _ => return Err("alarm labels: an alarm is labelled neither acted nor false".to_owned()),
            };
            Ok(Label { source: source.to_owned(), at: at.unix_seconds(), acted, narrowed: field("narrowed").map(str::to_owned) })
        })
        .collect()
}

pub fn compute(alarms: &[Alarm], labels: &[Label], since: i64) -> BTreeMap<&'static str, Source> {
    SOURCES
        .iter()
        .map(|source| {
            let alerts = alarms.iter().filter(|alarm| alarm.source == *source && i64::try_from(alarm.at_ms / 1000).is_ok_and(|at| at >= since)).count();
            let read: Vec<&Label> = labels.iter().filter(|label| label.source == *source && label.at >= since).collect();
            let falses = read.iter().filter(|label| !label.acted).count();
            let narrowed = read.iter().rev().find(|label| !label.acted).and_then(|label| label.narrowed.clone());
            (*source, Source { alerts, labelled: read.len(), falses, narrowed })
        })
        .collect()
}

fn share(source: &Source) -> String {
    if source.labelled == 0 {
        return "unmeasured".to_owned();
    }
    format!("{}/{} ({:.0}%)", source.falses, source.labelled, 100.0 * source.falses as f64 / source.labelled as f64)
}

pub fn render(stats: &BTreeMap<&'static str, Source>, window: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "alarms, last {window}");
    let _ = writeln!(out, "{:<20} {:>7} {:>9} {:>6} {:>14}", "source", "alerts", "labelled", "false", "false share");
    for (name, source) in stats {
        let _ = writeln!(out, "{name:<20} {:>7} {:>9} {:>6} {:>14}", source.alerts, source.labelled, source.falses, share(source));
    }
    let _ = writeln!(out, "\nover {OVER_PERCENT}% false:");
    let over: Vec<_> = stats.iter().filter(|(_, source)| source.falses * 100 > source.labelled * OVER_PERCENT).collect();
    for (name, source) in &over {
        let _ = writeln!(out, "  {name}: {}", source.narrowed.as_deref().map_or_else(|| "not narrowed".to_owned(), |how| format!("narrowed: {how}")));
    }
    if over.is_empty() {
        let _ = writeln!(out, "  none");
    }
    out
}

#[cfg(test)]
#[path = "alarm_stats_tests.rs"]
mod tests;
