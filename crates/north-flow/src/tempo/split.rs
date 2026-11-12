//! `north ledger tempo --split`: where each landed lane's hand-in call spent its seconds, part by part. The call stamps
//! each part as a span in the repository's landing queue (`logs/spans.jsonl`, trace `enqueue:<head>`); a part with
//! parts of its own is split into them, so only the leaves are summed.
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::landing::percentiles;

const SPANS: &str = "logs/spans.jsonl";
const TRACE_PREFIX: &str = "enqueue:";

#[derive(Deserialize)]
struct Stamped {
    id: String,
    #[serde(default)]
    parent: Option<String>,
    trace: String,
    name: String,
    start: f64,
    end: f64,
}

/// One landed lane's hand-in call: its head and the seconds from the call to its ticket write.
pub struct Call {
    pub head: String,
    pub seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Part {
    pub name: String,
    pub p50_seconds: Option<f64>,
    pub p90_seconds: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Split {
    pub lanes: usize,
    pub call_p50_seconds: Option<f64>,
    pub call_p90_seconds: Option<f64>,
    pub parts: Vec<Part>,
    pub parts_p50_sum_seconds: f64,
}

pub fn read(queue: &Path, calls: &[Call]) -> Split {
    let spans = stamped(queue, calls);
    let (call_p50, call_p90) = percentiles(&calls.iter().map(|call| call.seconds).collect::<Vec<f64>>());
    let per_lane: Vec<BTreeMap<String, f64>> = calls.iter().map(|call| leaves(spans.get(&format!("{TRACE_PREFIX}{}", call.head)).map_or(&[][..], Vec::as_slice))).collect();
    let names: BTreeSet<&String> = per_lane.iter().flat_map(BTreeMap::keys).collect();
    let mut parts: Vec<Part> = names
        .into_iter()
        .map(|name| {
            let samples: Vec<f64> = per_lane.iter().map(|lane| lane.get(name).copied().unwrap_or(0.0)).collect();
            let (p50, p90) = percentiles(&samples);
            Part { name: name.clone(), p50_seconds: p50, p90_seconds: p90 }
        })
        .collect();
    parts.sort_by(|left, right| right.p50_seconds.unwrap_or(0.0).total_cmp(&left.p50_seconds.unwrap_or(0.0)));
    let parts_p50_sum_seconds = parts.iter().filter_map(|part| part.p50_seconds).sum();
    Split { lanes: calls.len(), call_p50_seconds: call_p50, call_p90_seconds: call_p90, parts, parts_p50_sum_seconds }
}

fn stamped(queue: &Path, calls: &[Call]) -> BTreeMap<String, Vec<Stamped>> {
    let wanted: HashSet<String> = calls.iter().map(|call| format!("{TRACE_PREFIX}{}", call.head)).collect();
    let text = fs::read_to_string(queue.join(SPANS)).unwrap_or_default();
    let mut spans: BTreeMap<String, Vec<Stamped>> = BTreeMap::new();
    for line in text.lines().filter(|line| line.contains("\"trace\":\"enqueue:")) {
        if let Ok(span) = serde_json::from_str::<Stamped>(line)
            && wanted.contains(&span.trace)
        {
            spans.entry(span.trace.clone()).or_default().push(span);
        }
    }
    spans
}

fn leaves(spans: &[Stamped]) -> BTreeMap<String, f64> {
    let parents: HashSet<&str> = spans.iter().filter_map(|span| span.parent.as_deref()).collect();
    let mut parts = BTreeMap::new();
    for span in spans.iter().filter(|span| !parents.contains(span.id.as_str())) {
        *parts.entry(span.name.clone()).or_insert(0.0) += (span.end - span.start).max(0.0);
    }
    parts
}

/// The split's rows: one per part, largest median first, then the call's median and how the part medians add up to it.
pub fn lines(split: &Split) -> Vec<String> {
    let figure = |seconds: Option<f64>| seconds.map_or_else(|| "n/a".into(), |seconds| format!("{seconds:.1} s"));
    let mut lines = vec![format!("split over {} lanes: call p50 {} p90 {}", split.lanes, figure(split.call_p50_seconds), figure(split.call_p90_seconds))];
    lines.extend(split.parts.iter().map(|part| format!("  {}: p50 {} p90 {}", part.name, figure(part.p50_seconds), figure(part.p90_seconds))));
    let share = split.call_p50_seconds.filter(|call| *call > 0.0).map_or_else(|| "n/a".into(), |call| format!("{:.0}%", split.parts_p50_sum_seconds / call * 100.0));
    lines.push(format!("  part medians sum to {:.1} s, {share} of the call median", split.parts_p50_sum_seconds));
    lines
}

#[cfg(test)]
#[path = "split_tests.rs"]
mod tests;
