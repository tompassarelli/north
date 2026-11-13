use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::land::telemetry::epoch;
use crate::land::slots::runner::{LINE_WAIT, SANDBOX_START, Window};
use crate::land::trace::{Category, PHASE, Span, Spans};

pub(super) const BEFORE_TICKET: &str = "enqueue";
const LANE: &str = "lane";
const TICKET: &str = "ticket";
const SETUP: &str = "before enqueue";
const UNSTEPPED: &str = "not in a step";
const LINE_WAITS: &str = "line_waits";

pub(super) struct Timings {
    called: f64,
    begun: f64,
    started: Instant,
    stages: Mutex<Vec<Stage>>,
}

struct Stage {
    name: &'static str,
    start: f64,
    took: Duration,
    steps: Vec<(&'static str, Duration)>,
    windows: Vec<Window>,
}

impl Timings {
    pub(super) fn start(called_micros: u128) -> Self {
        Self { called: called_micros as f64 / 1e6, begun: epoch(), started: Instant::now(), stages: Mutex::new(Vec::new()) }
    }

    pub(super) fn stage<T>(&self, name: &'static str, run: impl FnOnce() -> T) -> T {
        self.stage_in_steps(name, run, || (Vec::new(), Vec::new()))
    }

    pub(super) fn stage_in_steps<T>(&self, name: &'static str, run: impl FnOnce() -> T, steps: impl FnOnce() -> (Vec<(&'static str, Duration)>, Vec<Window>)) -> T {
        let (start, began) = (epoch(), Instant::now());
        let result = run();
        let took = began.elapsed();
        let (steps, windows) = steps();
        self.stages.lock().unwrap_or_else(PoisonError::into_inner).push(Stage { name, start, took, steps, windows });
        result
    }

    pub(super) fn line(&self) -> String {
        let stages: Vec<String> = self.stages.lock().unwrap_or_else(PoisonError::into_inner).iter().map(described).collect();
        format!("safe-push: timings: total {}: {}", seconds(self.started.elapsed()), stages.join(", "))
    }

    pub(super) fn record(&self, logs: &Path, head: &str, lane: &str, ticket: &str, host: &str) {
        let trace = format!("{BEFORE_TICKET}:{head}");
        let stamp = |span: Span| span.on(host).with(PHASE, BEFORE_TICKET).with(LANE, lane).with(TICKET, ticket);
        let setup = (self.called > 0.0 && self.begun > self.called).then(|| stamp(Span::new(&trace, SETUP, category(SETUP), self.called, self.begun)));
        let spans: Vec<Span> = setup.into_iter().chain(self.stages.lock().unwrap_or_else(PoisonError::into_inner).iter().flat_map(|stage| stage_spans(&trace, stage, &stamp))).collect();
        if let Err(error) = Spans::beside(logs).append(&spans) {
            println!("safe-push: could not record the checks before the ticket: {error}");
        }
    }
}

fn stage_spans(trace: &str, stage: &Stage, stamp: &impl Fn(Span) -> Span) -> Vec<Span> {
    let stage_end = stage.start + stage.took.as_secs_f64();
    let waits = windows(trace, stage, LINE_WAIT, Category::HostWait, stamp);
    let parent = Span::new(trace, stage.name, category(stage.name), stage.start, stage_end);
    let parent = stamp(if waits.is_empty() { parent } else { parent.with(LINE_WAITS, waits.len().to_string()) });
    let starts = windows(trace, stage, SANDBOX_START, Category::Offload, stamp);
    let mut spans: Vec<Span> = waits.into_iter().chain(starts).map(|window| window.under(&parent.id)).collect();
    let mut at = stage.start;
    for (name, took) in &stage.steps {
        let end = (at + took.as_secs_f64()).min(stage_end);
        spans.push(stamp(Span::new(trace, format!("{} / {name}", stage.name), category(stage.name), at, end)).under(&parent.id));
        at = end;
    }
    if !stage.steps.is_empty() && stage_end > at {
        spans.push(stamp(Span::new(trace, format!("{} / {UNSTEPPED}", stage.name), category(stage.name), at, stage_end)).under(&parent.id));
    }
    spans.insert(0, parent);
    spans
}

fn windows(trace: &str, stage: &Stage, kind: &str, category: Category, stamp: &impl Fn(Span) -> Span) -> Vec<Span> {
    stage.windows.iter().filter(|(what, _, _)| *what == kind).map(|(_, from, to)| stamp(Span::new(trace, format!("{} / {kind}", stage.name), category, stage.start + from, stage.start + to))).collect()
}

fn category(stage: &str) -> Category {
    match stage {
        "fetch" | SETUP => Category::Git,
        "secret scan" | "private-kind ask" => Category::Scans,
        "ticket" => Category::Queue,
        _ => Category::Check,
    }
}

fn described(stage: &Stage) -> String {
    if stage.steps.is_empty() && stage.windows.is_empty() {
        return format!("{} {}", stage.name, seconds(stage.took));
    }
    let mut windows = stage.windows.clone();
    windows.sort_by(|left, right| left.1.total_cmp(&right.1));
    let ran = windows.iter().map(|(what, from, to)| format!("{what} ran {from:.2}-{to:.2} s"));
    let steps: Vec<String> = stage.steps.iter().map(|(name, took)| format!("{name} {}", seconds(*took))).chain(ran).collect();
    format!("{} {} ({})", stage.name, seconds(stage.took), steps.join(", "))
}

fn seconds(took: Duration) -> String {
    format!("{:.2} s", took.as_secs_f64())
}
