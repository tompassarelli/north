use std::collections::BTreeSet;

use serde_json::Value;

use super::estimate::{Scale, box_eta};
use super::metrics::{Metrics, delivery_metrics};
use super::plan::{gate_width, snapshot};
use super::schedule::{Task, schedule};
use super::{plus_minutes, read_config, snapshot_path};
use crate::foundation::clock::Timestamp;
use crate::threads::calibration::box_durations;
use crate::threads::verb::Failure;

struct Options {
    project: String,
    boxes: usize,
    category: Option<String>,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let (mut project, mut boxes, mut category, mut quote) = (None, None, None, false);
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = || rest.next().cloned().ok_or_else(|| format!("{argument} needs a value"));
        match argument.as_str() {
            "--quote" => quote = true,
            "--project" => project = Some(value()?),
            "--category" => category = Some(value()?),
            "--boxes" => boxes = Some(value()?.parse::<i64>().map_err(|_| "--boxes must be an integer".to_owned())?),
            other => return Err(format!("unrecognized arguments: {other}")),
        }
    }
    let (project, boxes) = (project.ok_or("--project is required")?, boxes.ok_or("--boxes is required")?);
    if !quote {
        return Err("--quote is required".into());
    }
    if boxes < 1 {
        return Err("--boxes must be positive".into());
    }
    let named = project.chars().next().is_some_and(|first| first.is_ascii_alphanumeric()) && project.chars().all(|letter| letter.is_ascii_alphanumeric() || "_.-".contains(letter));
    if !named {
        return Err("--project must be a project name".into());
    }
    Ok(Options { project, boxes: boxes as usize, category })
}

fn python_float(value: f64) -> String {
    let text = value.to_string();
    if text.contains('.') || text.contains("inf") || text.contains("NaN") { text } else { format!("{text}.0") }
}

fn shown(value: Option<f64>) -> String {
    value.map_or("None".to_owned(), python_float)
}

/// Date by the critical path of `boxes` independent boxes at the spawn gate's width, each box the category cell's calibrated ETA (or
/// the uncalibrated project median) plus the landing overhead; dollars are the median per done box.
pub struct Quote<'a> {
    pub project: &'a str,
    pub boxes: usize,
    pub metrics: &'a Metrics,
    pub now: Timestamp,
    pub width: usize,
    pub landing: f64,
    pub eta: Option<(Option<f64>, u64, String)>,
}

impl Quote<'_> {
    pub fn lines(&self) -> Result<(Vec<String>, String, String), String> {
        let measured = self.eta.as_ref().and_then(|found| found.0);
        let minutes = measured.or(self.metrics.median_brief_to_landed_min);
        let total = match minutes {
            Some(minutes) => {
                let tasks: Vec<Task> = (0..self.boxes).map(|at| Task { key: at.to_string(), eta: minutes, deps: BTreeSet::new() }).collect();
                Some(schedule(&tasks, self.width, self.landing)?)
            }
            None => None,
        };
        let date = total.map_or("n/a".to_owned(), |total| plus_minutes(self.now, total).iso());
        let dollars = self.metrics.median_cost_per_done_usd.map_or("n/a".to_owned(), |cost| format!("{:.2}", cost * self.boxes as f64));
        let (n, source) = self.eta.as_ref().map_or((0, "uncalibrated: project median brief-to-landed".to_owned()), |found| (found.1, found.2.clone()));
        let per_box = minutes.map_or("n/a".to_owned(), |minutes| if measured.is_some() { format!("{minutes}") } else { python_float(minutes) });
        let line = format!(
            "project {}: {} boxes, date {date}, ${dollars}; minutes/box {per_box} (n={n}, {source}), width {}, landing {}m/box, median minutes/box {}, concurrency {}, done boxes {}",
            self.project, self.boxes, self.width, self.landing, shown(self.metrics.median_brief_to_landed_min), self.metrics.concurrency, self.metrics.done_boxes,
        );
        Ok((vec![line], date, dollars))
    }
}

fn category_eta(category: &str) -> Result<(Option<f64>, u64, String), String> {
    let facets = [format!("Category: {category}")];
    let measured = box_durations(&snapshot_path(), Timestamp::now(), &facets, None).map_err(|failure| match failure {
        Failure::Usage(why) | Failure::Refused(why) => why,
    })?;
    let label = measured["label"].as_str().unwrap_or("uncalibrated: no box row");
    let (eta, n, source) = box_eta(&measured["boxes"][0], &Scale { value: measured["scale"].as_f64(), label }, None);
    Ok((eta, n, source))
}

fn run_quote(options: &Options) -> Result<Option<String>, String> {
    let config = read_config(&crate::pace::target::policy_path())?;
    let Some(project) = config["projects"].get(&options.project) else { return Ok(Some(format!("unknown project '{}'", options.project))) };
    let database = snapshot_path();
    if database.file_name().is_some_and(|name| name == "threads.db") {
        return Err("quotes require the mirrored snapshot.db, not threads.db".into());
    }
    let rows: Vec<Value> = snapshot(&database).unwrap_or_default();
    let metrics = delivery_metrics(&rows, Some(&options.project));
    let eta = options.category.as_deref().map(category_eta).transpose()?;
    let (width, _) = gate_width(delivery_metrics(&rows, None).concurrency);
    let landing = if eta.as_ref().is_some_and(|found| found.0.is_some()) { 0.0 } else { crate::ops::flow::landing_minutes(&options.project) };
    let quote = Quote { project: &options.project, boxes: options.boxes, metrics: &metrics, now: Timestamp::now(), width, landing, eta };
    let (mut lines, date, dollars) = quote.lines()?;
    if project.get("target").is_some() {
        lines.push(format!("Blocked: decision for Tom: {} date {date} ${dollars} [approve|later date|smaller scope]", options.project));
    }
    println!("{}", lines.join("\n"));
    Ok(None)
}

pub fn run(arguments: &[String]) -> u8 {
    let options = match parse(arguments) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("usage: agents plan --quote --project P --boxes N [--category C]\nagents plan --quote: error: {error}");
            return 2;
        }
    };
    match run_quote(&options) {
        Ok(None) => 0,
        Ok(Some(error)) => {
            eprintln!("agents plan --quote: error: {error}");
            2
        }
        Err(error) => {
            eprintln!("agents plan --quote: {error}");
            1
        }
    }
}
