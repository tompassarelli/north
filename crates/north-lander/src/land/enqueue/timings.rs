use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

pub(super) struct Timings {
    started: Instant,
    stages: Mutex<Vec<(&'static str, Duration)>>,
}

impl Timings {
    pub(super) fn start() -> Self {
        Self { started: Instant::now(), stages: Mutex::new(Vec::new()) }
    }

    pub(super) fn stage<T>(&self, name: &'static str, run: impl FnOnce() -> T) -> T {
        let began = Instant::now();
        let result = run();
        self.stages.lock().unwrap_or_else(PoisonError::into_inner).push((name, began.elapsed()));
        result
    }

    pub(super) fn line(&self) -> String {
        let stages: Vec<String> = self.stages.lock().unwrap_or_else(PoisonError::into_inner).iter().map(|(name, took)| format!("{name} {}", seconds(*took))).collect();
        format!("safe-push: timings: total {}: {}", seconds(self.started.elapsed()), stages.join(", "))
    }
}

fn seconds(took: Duration) -> String {
    format!("{:.2} s", took.as_secs_f64())
}
