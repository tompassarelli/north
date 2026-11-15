//! The survey keeper: while launches place work, one process per placing host asks every build host what placement reads each
//! `EVERY`, with a saturation reading taken then, so a launch finds an answer under 30 s old instead of asking the host itself,
//! which cost a launch to a build host about 1 s before it registered on 2026-11-12. It runs apart from the launch that started
//! it, stops once no launch has placed work for `IDLE_SECONDS`, and hands its work to a keeper of the current runtime once the one it started from is no longer current.
use std::fs::{self, File, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::super::storage;

const EVERY: Duration = Duration::from_secs(20);
/// A guess at the longest quiet stretch between launches of a working day, which the share of launches that find a cold survey would settle: at 300 s the first of five launches at 6:56 AM on 2026-11-15 waited 2000 ms on the survey and 1694 ms on the spend view (north#1531).
const IDLE_SECONDS: f64 = 4.0 * 60.0 * 60.0;
const USAGE: &str = "Usage: north placement-survey keep";

/// Marks that a launch is placing work, and starts the keeper when none runs.
pub(super) fn wanted() {
    if std::env::var_os("PLACEMENT_HOSTS").is_some() {
        return;
    }
    let dir = directory();
    if fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join("wanted"), storage::now().to_string())).is_err() {
        return;
    }
    let Some(lock) = lock_file(&dir) else { return };
    if lock.try_lock().is_err() {
        return;
    }
    drop(lock);
    let Ok(program) = std::env::current_exe() else { return };
    let _ = Command::new(program).args(["placement-survey", "keep"]).process_group(0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// `north placement-survey keep`: keeps every build host's survey answer fresh until launches stop placing work; a keeper already running leaves this one nothing to do.
pub fn run(arguments: &[String], refresh_every_host: impl Fn()) -> u8 {
    if arguments != ["keep"] {
        eprintln!("{USAGE}");
        return 2;
    }
    let dir = directory();
    let Some(lock) = fs::create_dir_all(&dir).ok().and_then(|()| lock_file(&dir)) else { return 1 };
    if lock.try_lock().is_err() {
        return 0;
    }
    let started_on = current_runtime();
    while storage::now() - wanted_at(&dir) < IDLE_SECONDS {
        if current_runtime() != started_on {
            drop(lock);
            let _ = Command::new(crate::platform::north_binary::north_binary()).args(["placement-survey", "keep"]).process_group(0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
            return 0;
        }
        refresh_every_host();
        std::thread::sleep(EVERY);
    }
    0
}

fn directory() -> PathBuf {
    storage::directory().join("survey")
}

fn lock_file(dir: &Path) -> Option<File> {
    OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("keeper.lock")).ok()
}

fn wanted_at(dir: &Path) -> f64 {
    fs::read_to_string(dir.join("wanted")).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0.0)
}

fn current_runtime() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from);
    fs::canonicalize(state.join("north/current")).ok()
}
