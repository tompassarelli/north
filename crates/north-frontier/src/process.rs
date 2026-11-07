use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};

pub fn spawn(command: &mut Command) -> std::io::Result<Child> {
    command.process_group(0).spawn()
}

pub fn kill(child: &mut Child) {
    let Some(group) = Pid::from_raw(child.id() as i32) else { return };
    let _ = kill_process_group(group, Signal::Term);
    let grace = Instant::now() + Duration::from_secs(15);
    while Instant::now() < grace && !matches!(child.try_wait(), Ok(Some(_))) {
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = kill_process_group(group, Signal::Kill);
    let _ = child.wait();
}

/// Waits for the child until the deadline; `None` means it was still running and has been killed.
pub fn wait_until(child: &mut Child, deadline: Instant) -> Option<i32> {
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status.code().unwrap_or(-1));
        }
        if Instant::now() >= deadline {
            kill(child);
            return None;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The prefix that runs a command inside a heavy machine-capacity lease on this host, so oracle and
/// grading test runs wait behind land checks instead of competing with them.
pub fn leased(owner: &str, bound: Duration) -> Result<Vec<String>, north_base::error::NorthError> {
    let north = std::env::current_exe().map_err(north_base::error::NorthError::Io)?.display().to_string();
    Ok(vec![north.trim_end_matches(" (deleted)").to_owned(), "capacity".into(), "host".into(), "run".into(), "--class".into(), "heavy".into(), "--owner".into(), owner.into(), "--timeout-seconds".into(), bound.as_secs().to_string(), "--".into()])
}
