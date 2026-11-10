use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use super::client::North;
use super::exec;
use super::notes::{Manifest, Notes};
use super::processes::{self, Process};
use super::report;
use super::journey;
use super::scratch;

pub const ARGUMENT: &str = "--janitor";
const SETTLE: Duration = Duration::from_secs(20);
const CHILD_GRACE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(200);

pub struct Janitor {
    child: Child,
    notes: Notes,
}

impl Janitor {
    pub fn spawn(state: PathBuf) -> io::Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg(ARGUMENT)
            .env("XDG_STATE_HOME", &state)
            .env("NORTH_TEST_ROOT", &state)
            .stdin(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let notes = Notes::new(child.stdin.take());
        Ok(Self { child, notes })
    }

    pub fn notes(&self) -> Notes {
        self.notes.clone()
    }

    pub fn finish(mut self) -> bool {
        self.notes.close();
        self.child.wait().is_ok_and(|status| status.success())
    }
}

pub fn serve() -> ExitCode {
    let manifest = Manifest::read(io::stdin().lock());
    let started = Instant::now();
    let outcome = sweep(&manifest);
    report::step("5", "cleanup: forget the worker, remove the scratch repository and its landing queue, no processes left", &outcome, started.elapsed());
    if outcome.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn sweep(manifest: &Manifest) -> Result<String, String> {
    let mut started: Vec<Process> = manifest.children.iter().copied().collect();
    started.extend(manifest.scratch.as_deref().map(processes::follows).unwrap_or_default());
    started.extend(processes::descendants(&started));
    for process in &started {
        process.terminate();
    }
    settle(&started, CHILD_GRACE);
    let mut watched: Vec<Process> = manifest.watched.iter().copied().collect();
    watched.extend(processes::descendants(&watched));
    let mut problems = forget_all(manifest);
    if let Some(root) = &manifest.scratch {
        watched.extend(processes::within(root));
    }
    let left = settle(&watched, SETTLE);
    if !left.is_empty() {
        let names: Vec<String> = left.iter().map(|process| format!("{} {}", process.pid, process.command_line())).collect();
        problems.push(format!("{} processes outlived cleanup and were terminated: {}", left.len(), names.join(" | ")));
        for process in &left {
            process.terminate();
        }
    }
    if let (Some(state), Some(queue)) = (&manifest.state, &manifest.queue) {
        problems.extend(remove(queue, queue.parent() == Some(journey::landing_queues(state).as_path())));
    }
    problems.extend(manifest.scratch.as_deref().and_then(|root| remove(root, scratch::removable(root))));
    problems.extend(manifest.state.as_deref().and_then(|root| remove(root, scratch::removable(root))));
    if problems.is_empty() {
        Ok(format!("{} sessions forgotten, {} processes gone", manifest.sessions.len(), watched.len()))
    } else {
        Err(problems.join("; "))
    }
}

fn forget_all(manifest: &Manifest) -> Vec<String> {
    let (Some(program), Some(directory), Some(state)) = (&manifest.north, &manifest.directory, &manifest.state) else {
        return manifest.sessions.iter().map(|name| format!("cannot forget {name}: no north binary or state root was recorded")).collect();
    };
    let north = North::new(program.clone(), directory.clone(), state.clone(), None);
    manifest
        .sessions
        .iter()
        .filter_map(|name| match north.forget(name) {
            Ok(()) => None,
            Err(error) if error.contains("no session named") => None,
            Err(error) => Some(format!("forget {name}: {error}")),
        })
        .collect()
}

fn settle(processes: &[Process], limit: Duration) -> Vec<Process> {
    exec::poll_until(limit, POLL, || processes.iter().all(|process| !process.alive()));
    processes.iter().copied().filter(|process| process.alive()).collect()
}

fn remove(path: &Path, owned: bool) -> Option<String> {
    if !owned {
        return Some(format!("refused to remove {}: not a directory verify-north creates", path.display()));
    }
    fs::remove_dir_all(path).err().filter(|error| error.kind() != io::ErrorKind::NotFound).map(|error| format!("remove {}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
