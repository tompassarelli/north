use std::collections::BTreeSet;

use serde_json::Value;

use super::stamp;
use crate::foundation::clock::Timestamp;
use crate::threads::calibration::{BoxRun, Knobs, box_groups, box_run};

#[derive(Debug, PartialEq)]
pub struct Metrics {
    pub done_boxes: usize,
    pub concurrency: usize,
    pub median_brief_to_landed_min: Option<f64>,
    pub median_cost_per_done_usd: Option<f64>,
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let middle = values.len() / 2;
    match values.len() {
        0 => None,
        count if count % 2 == 1 => Some(values[middle]),
        _ => Some((values[middle - 1] + values[middle]) / 2.0),
    }
}

fn text<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row[key].as_str().filter(|value| !value.is_empty())
}

fn begin(row: &Value) -> Option<Timestamp> {
    stamp(text(row, "staffed").or_else(|| text(row, "started"))?)
}

fn concurrency(rows: &[&Value]) -> usize {
    let mut events: Vec<(i128, i64)> = Vec::new();
    for row in rows {
        if let (Some(start), Some(end)) = (begin(row), text(row, "ended").and_then(stamp)) {
            events.extend([(start.unix_nanos(), 1), (end.unix_nanos(), -1)]);
        }
    }
    events.sort_unstable();
    let (mut active, mut widest) = (0_i64, 1_i64);
    for (_, delta) in events {
        active += delta;
        widest = widest.max(active);
    }
    widest as usize
}

fn costs(rows: &[Value], knobs: &Knobs) -> Vec<f64> {
    let runs: Vec<BoxRun> = rows.iter().filter_map(box_run).collect();
    box_groups(&runs, knobs.box_chain_gap_min)
        .iter()
        .filter(|group| group.iter().all(|(_, run)| run.cost_usd.is_some()))
        .map(|group| group.iter().filter_map(|(_, run)| run.cost_usd).sum())
        .collect()
}

/// What the worker ledger measures of delivery: landed done boxes, the most runs staffed at once, the median minutes from brief to
/// landing and the median dollars of a landed box, over one project's rows or all of them.
pub fn delivery_metrics(rows: &[Value], project: Option<&str>) -> Metrics {
    let rows: Vec<Value> = rows.iter().filter(|row| project.is_none_or(|project| text(row, "item").unwrap_or("").split('#').next() == Some(project))).cloned().collect();
    let costs = costs(&rows, &Knobs::registry());
    let done: Vec<&Value> = rows.iter().filter(|row| row["role"].as_str().unwrap_or("worker") == "worker" && row["outcome"] == "done" && row["landed"] == 1).collect();
    let concurrency = concurrency(&done);
    let followed: BTreeSet<Option<&str>> = done.iter().map(|row| row["follows"].as_str()).collect();
    let done: Vec<&Value> = done.into_iter().filter(|row| !followed.contains(&row["agent"].as_str())).collect();
    let durations: Vec<f64> = done
        .iter()
        .filter_map(|row| Some((stamp(text(row, "landed_at")?)?.unix_nanos() - begin(row)?.unix_nanos()) as f64 / 60e9))
        .collect();
    Metrics { done_boxes: done.len(), concurrency, median_brief_to_landed_min: median(durations), median_cost_per_done_usd: median(costs) }
}
