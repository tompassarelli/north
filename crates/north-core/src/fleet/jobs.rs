//! The records `job-run --notify` keeps under `north/jobs`: each names the fleet session its exit is
//! sent to, and the detached job holds a lock on it until it exits, so a record is live while it is locked.
use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};

const EXTENSION: &str = "job";

pub struct Jobs {
    dir: PathBuf,
}

impl Jobs {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn in_state() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
        Self::new(state.unwrap_or_else(home).join("north/jobs"))
    }

    /// Whether a job still running will send its exit to `session`.
    pub fn running_for(&self, session: &str) -> bool {
        let mut records = fs::read_dir(&self.dir).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == EXTENSION));
        records.any(|path| addressed_to(&path, session) && running(&path))
    }
}

fn addressed_to(record: &Path, session: &str) -> bool {
    fs::read_to_string(record).is_ok_and(|text| text.lines().next() == Some(session))
}

fn running(record: &Path) -> bool {
    File::open(record).is_ok_and(|file| matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(dir: &Path, name: &str, session: &str) -> File {
        let path = dir.join(format!("{name}.job"));
        fs::write(&path, format!("{session}\n")).unwrap();
        File::options().append(true).open(path).unwrap()
    }

    #[test]
    fn a_job_holding_its_record_is_running_for_the_session_it_names_and_for_no_other() {
        let dir = tempfile::tempdir().unwrap();
        let held = record(dir.path(), "push-1", "kid");
        held.lock().unwrap();
        let jobs = Jobs::new(dir.path().to_path_buf());
        assert!(jobs.running_for("kid"));
        assert!(!jobs.running_for("other"));
    }

    #[test]
    fn a_job_that_released_its_record_is_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let held = record(dir.path(), "push-1", "kid");
        held.lock().unwrap();
        drop(held);
        assert!(!Jobs::new(dir.path().to_path_buf()).running_for("kid"));
        assert!(!Jobs::new(dir.path().join("missing")).running_for("kid"));
    }
}
