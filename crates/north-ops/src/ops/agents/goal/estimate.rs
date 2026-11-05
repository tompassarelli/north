use serde_json::Value;

use super::boxes::{GoalBox, parse_boxes};
use super::schedule::{Task, schedule, serial};
use super::{plus_minutes, span, stamp};
use crate::foundation::clock::Timestamp;

pub struct Estimate {
    pub project: String,
    pub boxes: usize,
    pub width: usize,
    pub minutes: Option<f64>,
    pub finish: String,
    pub serial_minutes: f64,
    pub serial_finish: String,
    pub ns: Vec<String>,
    pub detail: Vec<GoalBox>,
}

pub struct Scale<'a> {
    pub value: Option<f64>,
    pub label: &'a str,
}

/// (minutes, n, source): the measured box p75 scaled by the box calibration; with no chains, the project median brief-to-landed.
pub fn box_eta(measured: &Value, scale: &Scale, fallback: Option<f64>) -> (Option<f64>, u64, String) {
    let (eta, n, used) = (measured["minutes"].as_f64().filter(|eta| *eta != 0.0), measured["n"].as_u64().unwrap_or(0), measured["source"].as_str().unwrap_or(""));
    if let (Some(eta), true) = (eta, n > 0) {
        let scaled = scale.value.filter(|value| *value != 0.0).map_or(eta, |value| eta * value);
        return (Some(scaled.ceil()), n, format!("{used} {}", scale.label));
    }
    match fallback {
        Some(median) => (Some(median.ceil()), 0, "uncalibrated: project median brief-to-landed".to_owned()),
        None => (None, 0, "uncalibrated: no box chains and no project median".to_owned()),
    }
}

/// Critical path over the goal's boxes; a box chain already ends at its landing, so no landing overhead is added. `measure` gives the
/// box ledger's durations for the boxes' facet texts.
pub fn estimate(project: &str, issues: &[Value], measure: &dyn Fn(&[String]) -> Result<Value, String>, median: Option<f64>, width: usize, now: Timestamp) -> Result<Estimate, String> {
    let mut boxes = parse_boxes(project, issues);
    let facets: Vec<String> = boxes.iter().map(|found| found.facets.clone()).collect();
    let measured = if boxes.is_empty() { Value::Null } else { measure(&facets)? };
    let label = measured["label"].as_str().unwrap_or("uncalibrated: no box row");
    let scale = Scale { value: measured["scale"].as_f64(), label };
    let mut ns = Vec::new();
    for (found, one) in boxes.iter_mut().zip(measured["boxes"].as_array().into_iter().flatten()) {
        let (eta, n, source) = box_eta(one, &scale, median);
        (found.eta, found.source) = (eta, source);
        ns.push(if n > 0 { n.to_string() } else { "uncal".to_owned() });
    }
    let mut result = Estimate { project: project.to_owned(), boxes: boxes.len(), width, minutes: None, finish: String::new(), serial_minutes: 0.0, serial_finish: String::new(), ns, detail: Vec::new() };
    if boxes.iter().any(|found| found.eta.is_none()) {
        result.detail = boxes;
        return Ok(result);
    }
    let tasks: Vec<Task> = boxes.iter().map(|found| Task { key: found.key.clone(), eta: found.eta.unwrap_or_default(), deps: found.deps.clone() }).collect();
    let minutes = if tasks.is_empty() { 0.0 } else { schedule(&tasks, width, 0.0)? };
    let hand = serial(&tasks, 0.0);
    result.minutes = Some(minutes);
    result.finish = plus_minutes(now, minutes).iso();
    result.serial_minutes = hand;
    result.serial_finish = plus_minutes(now, hand).iso();
    result.detail = boxes;
    Ok(result)
}

pub fn line(result: &Estimate, target: Option<&str>) -> String {
    let project = &result.project;
    if result.boxes == 0 {
        return format!("goal {project}: no open goal boxes");
    }
    let Some(minutes) = result.minutes else {
        return format!("goal {project}: ETA n/a over {} boxes (a box has no category cell and no project median)", result.boxes);
    };
    let verdict = target.and_then(stamp).and_then(|target| Some((target, stamp(&result.finish)?))).map_or(String::new(), |(target, finish)| {
        let slack = (target.unix_nanos() - finish.unix_nanos()) as f64 / 60e9;
        format!(" vs target {} ({} {})", target.iso(), span(slack.abs()), if slack >= 0.0 { "early" } else { "late" })
    });
    let ns = if result.ns.is_empty() { "-".to_owned() } else { result.ns.join(",") };
    format!(
        "goal {project}: ETA {} critical path over {} boxes at width {} (box = first staffed to last landed), finish {}{verdict}; serial {}; n per box {ns}",
        span(minutes),
        result.boxes,
        result.width,
        result.finish,
        span(result.serial_minutes),
    )
}
