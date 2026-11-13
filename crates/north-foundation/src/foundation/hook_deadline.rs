use super::clock::Timestamp;
use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

/// The seconds the runtime lets a hook run before it ends the hook and lets the call through; the generated hook wiring sets it on every hook (nixos-config modules/north-profile/claude-hooks.json).
pub const HOOK_TIMEOUT_SECS: u32 = 60;

/// `work`'s answer when it comes before `deadline`, else None. The deadline bounds the hook's own time, running or blocked on what it waits for; the time a loaded host left it runnable but off a CPU moves the deadline later, up to the runtime's [`HOOK_TIMEOUT_SECS`], so a busy host does not turn a guard's answer into an empty one (north#1296).
pub fn within<T: Send + 'static>(deadline: Instant, work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let ceiling = Instant::now() + Duration::from_secs(HOOK_TIMEOUT_SECS.into());
    let starved_before = run_delay();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    let mut until = deadline;
    loop {
        match receiver.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(done) => return Some(done),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let moved = (deadline + run_delay().saturating_sub(starved_before)).min(ceiling);
                if moved <= until {
                    return None;
                }
                until = moved;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
                Err(panic) => std::panic::resume_unwind(panic),
                Ok(()) => return None,
            },
        }
    }
}

fn run_delay() -> Duration {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return Duration::ZERO;
    };
    // proc(5) schedstat format: the second field is the nanoseconds this thread waited runnable on a run queue.
    Duration::from_nanos(tasks.flatten().filter_map(|task| std::fs::read_to_string(task.path().join("schedstat")).ok()).filter_map(|line| line.split_whitespace().nth(1)?.parse::<u64>().ok()).sum())
}

pub fn errors_file() -> PathBuf {
    env::var_os("AGENT_HOOK_ERRORS").map_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state/agents/hooks/errors.tsv"), PathBuf::from)
}

pub fn record_error(file: &Path, hook: &str, kind: &str) {
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(out, "{}\t{hook}\t{kind}", Timestamp::now().iso());
    }
}
