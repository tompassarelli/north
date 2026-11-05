use super::clock::Timestamp;
use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Instant,
};

pub fn within<T: Send + 'static>(deadline: Instant, work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(done) => Some(done),
        Err(mpsc::RecvTimeoutError::Timeout) => None,
        Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => None,
        },
    }
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
