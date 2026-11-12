use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::land::trace::check_log::{self, Mark};
use super::spawn::Ran;

static STEPS: Mutex<Vec<(&'static str, Duration)>> = Mutex::new(Vec::new());
static RECORDING: AtomicBool = AtomicBool::new(false);

pub(super) fn begin() {
    STEPS.lock().unwrap_or_else(PoisonError::into_inner).clear();
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

/// Splits a check's own process into the capacity lease, the sandbox's start, the command and the release, by the times its lease lines and its first and last output lines reached the log; a check whose output names no lease counts whole as the command.
pub(super) fn split(ran: &Ran) -> Vec<(&'static str, Duration)> {
    let at = |wanted: fn(&str) -> bool| ran.marks.iter().find(|mark| wanted(&mark.line)).map(|mark| mark.at);
    let (Some(reserved), Some(released)) = (at(check_log::is_reserved), at(check_log::is_released)) else {
        return vec![("command", seconds(ran.ended_at - ran.started_at))];
    };
    let running = at(check_log::is_offload_running).filter(|running| *running <= reserved);
    let output: Vec<&Mark> = ran.marks.iter().filter(|mark| !check_log::is_marker(&mark.line) && mark.at >= reserved && mark.at <= released).collect();
    let first = output.first().map_or(reserved, |mark| mark.at);
    let last = output.last().map_or(first, |mark| mark.at);
    let slot = running.map(|running| ("slot and tree proof", seconds(running - ran.started_at)));
    let leased_from = running.unwrap_or(ran.started_at);
    slot.into_iter().chain([("lease", seconds(reserved - leased_from)), ("sandbox start", seconds(first - reserved)), ("command", seconds(last - first)), ("release", seconds(ran.ended_at - last))]).collect()
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
