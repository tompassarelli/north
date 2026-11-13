use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::land::trace::check_log::{self, Mark};
use crate::land::slots::graph_selection;

static STEPS: Mutex<Vec<(&'static str, Duration)>> = Mutex::new(Vec::new());
static RECORDING: AtomicBool = AtomicBool::new(false);
static WINDOWS: Mutex<(f64, Vec<Window>)> = Mutex::new((0.0, Vec::new()));

/// One check's run inside the checks before queueing: what it ran and when it started and ended, in seconds from the steps' beginning.
pub type Window = (&'static str, f64, f64);

pub const LINE_WAIT: &str = "line wait";
pub const SANDBOX_START: &str = "sandbox start";

pub(super) fn begin() {
    STEPS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    let _ = graph_selection::listing_waits();
    *WINDOWS.lock().unwrap_or_else(PoisonError::into_inner) = (crate::land::telemetry::epoch(), Vec::new());
    RECORDING.store(true, Ordering::Relaxed);
}

fn add(name: &'static str, took: Duration) {
    if !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    STEPS.lock().unwrap_or_else(PoisonError::into_inner).push((name, took));
}

fn seconds(seconds: f64) -> Duration {
    Duration::try_from_secs_f64(seconds).unwrap_or_default()
}

pub(super) fn timed<T>(name: &'static str, run: impl FnOnce() -> T) -> T {
    let began = Instant::now();
    let value = run();
    add(name, began.elapsed());
    value
}

pub(super) async fn awaited<T>(name: &'static str, work: impl Future<Output = T>) -> T {
    let began = Instant::now();
    let value = work.await;
    add(name, began.elapsed());
    value
}

/// Splits a check's own process into its slot and tree proof, the capacity lease, the sandbox's start, the command and the release, by the times its lease lines and its first and last output lines reached the log, and records the sandbox's start, between the lease and the command, as the check's own window; an offload run's timed line and its lease's RELEASED line name the parts of its proof and of its start, timed on the build host, and a check whose output names no lease counts whole as the command.
pub(super) fn split(started_at: f64, ended_at: f64, marks: &[Mark]) -> Vec<(&'static str, Duration)> {
    let at = |wanted: fn(&str) -> bool| marks.iter().find(|mark| wanted(&mark.line)).map(|mark| mark.at);
    let (Some(reserved), Some(released)) = (at(check_log::is_reserved), at(check_log::is_released)) else {
        return vec![("command", seconds(ended_at - started_at))];
    };
    let running = at(check_log::is_offload_running).filter(|running| *running <= reserved);
    let mut steps = Vec::new();
    if let Some(running) = running {
        match marks.iter().find_map(|mark| check_log::slot_steps(&mark.line)) {
            Some(parts) => {
                let remote: f64 = parts.iter().map(|(_, took)| took).sum();
                steps.push((check_log::SLOT_SHIP, seconds(running - started_at - remote)));
                steps.extend(parts.into_iter().map(|(name, took)| (name, seconds(took))));
            }
            None => steps.push(("slot and tree proof", seconds(running - started_at))),
        }
    }
    steps.push(("lease", seconds(reserved - running.unwrap_or(started_at))));
    if let Some(start) = marks.iter().find_map(|mark| check_log::lease_start(&mark.line)) {
        let starting: f64 = start.steps.iter().map(|(_, took)| took).sum();
        let ran = start.subject_seconds.unwrap_or_default();
        window(SANDBOX_START, reserved, reserved + starting);
        steps.extend(start.steps.into_iter().map(|(name, took)| (name, seconds(took))));
        steps.extend([("command", seconds(ran)), ("release", seconds(ended_at - reserved - starting - ran))]);
        return steps;
    }
    let output: Vec<&Mark> = marks.iter().filter(|mark| !check_log::is_marker(&mark.line) && mark.at >= reserved && mark.at <= released).collect();
    let first = output.first().map_or(reserved, |mark| mark.at);
    let last = output.last().map_or(first, |mark| mark.at);
    window(SANDBOX_START, reserved, first);
    steps.extend([("command", seconds(last - first)), ("release", seconds(ended_at - last))]);
    steps
}

pub(super) fn ran(command: &str, started_at: f64, ended_at: f64) {
    let what = if north_build_system::lints(command) { "clippy" } else if command.contains("nextest run") || command.contains("cargo test") || north_build_system::test_record(command).is_some() { "tests" } else { "check" };
    window(what, started_at, ended_at);
}

pub(super) fn waited(marks: &[Mark]) {
    for mark in marks {
        if let Some(waited) = check_log::waited_in_line(&mark.line) {
            in_line(mark.at - waited, mark.at);
        }
    }
}

fn in_line(started_at: f64, ended_at: f64) {
    window(LINE_WAIT, started_at, ended_at);
}

fn window(what: &'static str, started_at: f64, ended_at: f64) {
    if !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    let mut windows = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
    let began = windows.0;
    windows.1.push((what, started_at - began, ended_at - began));
}

/// The windows the checks before queueing ran in since the last `begin`, in the order they ended.
pub fn windows() -> Vec<Window> {
    let mut windows = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
    let began = windows.0;
    let listed = graph_selection::listing_waits().into_iter().map(|(started_at, ended_at)| (LINE_WAIT, started_at - began, ended_at - began));
    let mut taken = std::mem::take(&mut windows.1);
    taken.extend(listed);
    taken
}

pub(super) fn add_all(steps: Vec<(&'static str, Duration)>) {
    for (name, took) in steps {
        add(name, took);
    }
}

/// The steps the land check runner recorded in this process since the last call, in order, summed by name.
pub fn taken() -> Vec<(&'static str, Duration)> {
    RECORDING.store(false, Ordering::Relaxed);
    let steps = std::mem::take(&mut *STEPS.lock().unwrap_or_else(PoisonError::into_inner));
    let mut summed: Vec<(&'static str, Duration)> = Vec::new();
    for (name, took) in steps {
        match summed.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, total)) => *total += took,
            None => summed.push((name, took)),
        }
    }
    summed
}
