use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::land::telemetry::epoch;
use crate::land::trace::{Category, PHASE, Span, Spans};

pub(super) const BEFORE_TICKET: &str = "enqueue";

pub(super) struct Timings {
    started: Instant,
    stages: Mutex<Vec<Stage>>,
}

struct Stage {
    name: &'static str,
    start: f64,
    took: Duration,
    steps: Vec<(&'static str, Duration)>,
}

impl Timings {
    pub(super) fn start() -> Self {
        Self { started: Instant::now(), stages: Mutex::new(Vec::new()) }
    }

    pub(super) fn stage<T>(&self, name: &'static str, run: impl FnOnce() -> T) -> T {
        self.stage_in_steps(name, run, Vec::new)
    }

    pub(super) fn stage_in_steps<T>(&self, name: &'static str, run: impl FnOnce() -> T, steps: impl FnOnce() -> Vec<(&'static str, Duration)>) -> T {
        let (start, began) = (epoch(), Instant::now());
        let result = run();
        let took = began.elapsed();
        self.stages.lock().unwrap_or_else(PoisonError::into_inner).push(Stage { name, start, took, steps: steps() });
        result
    }

    pub(super) fn line(&self) -> String {
        let stages: Vec<String> = self.stages.lock().unwrap_or_else(PoisonError::into_inner).iter().map(described).collect();
        format!("safe-push: timings: total {}: {}", seconds(self.started.elapsed()), stages.join(", "))
    }

    pub(super) fn record(&self, logs: &Path, head: &str, host: &str) {
        let spans: Vec<Span> = self.stages.lock().unwrap_or_else(PoisonError::into_inner).iter().map(|stage| Span::new(&format!("{BEFORE_TICKET}:{head}"), stage.name, category(stage.name), stage.start, stage.start + stage.took.as_secs_f64()).on(host).with(PHASE, BEFORE_TICKET)).collect();
        if let Err(error) = Spans::beside(logs).append(&spans) {
            println!("safe-push: could not record the checks before the ticket: {error}");
        }
    }
}

fn category(stage: &str) -> Category {
    match stage {
        "fetch" => Category::Git,
        "secret scan" | "private-kind ask" => Category::Scans,
        "ticket" => Category::Queue,
        _ => Category::Check,
    }
}

fn described(stage: &Stage) -> String {
    if stage.steps.is_empty() {
        return format!("{} {}", stage.name, seconds(stage.took));
    }
    let steps: Vec<String> = stage.steps.iter().map(|(name, took)| format!("{name} {}", seconds(*took))).collect();
    format!("{} {} ({})", stage.name, seconds(stage.took), steps.join(", "))
}

fn seconds(took: Duration) -> String {
    format!("{:.2} s", took.as_secs_f64())
}
