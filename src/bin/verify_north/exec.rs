use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::notes::{Note, Notes};
use super::processes::Process;

const AGENT_VARIABLES: [&str; 2] = ["CLAUDE", "CODEX_HOME"];

pub fn operator_environment(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| AGENT_VARIABLES.iter().any(|prefix| key.starts_with(prefix))) {
            command.env_remove(key);
        }
    }
}

pub fn output(mut command: Command, timeout: Duration, notes: Option<&Notes>) -> Result<Output, String> {
    let label = describe(&command);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().map_err(|error| format!("{label}: {error}"))?;
    let process = Process::of(child.id());
    if let (Some(notes), Some(process)) = (notes, process) {
        notes.note(&Note::Child(process));
    }
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    let finished = receiver.recv_timeout(timeout);
    if finished.is_err() {
        process.into_iter().for_each(Process::terminate);
    }
    if let (Some(notes), Some(process)) = (notes, process) {
        notes.note(&Note::Reaped(process.pid));
    }
    match finished {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(format!("{label}: {error}")),
        Err(_) => Err(format!("{label} did not finish within {} s", timeout.as_secs())),
    }
}

pub fn stdout(command: Command, timeout: Duration, notes: Option<&Notes>) -> Result<String, String> {
    let label = describe(&command);
    let output = output(command, timeout, notes)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{label} exited {}: {}", output.status.code().unwrap_or(-1), stderr.trim()))
}

pub fn poll_until(limit: Duration, interval: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(interval);
    }
}

pub fn wait_for<T>(limit: Duration, interval: Duration, mut probe: impl FnMut() -> Result<Option<T>, String>) -> Result<Option<T>, String> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(found) = probe()? {
            return Ok(Some(found));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(interval);
    }
}

fn describe(command: &Command) -> String {
    let program = command.get_program().to_string_lossy();
    let program = program.rsplit('/').next().unwrap_or_default().to_owned();
    let words: Vec<String> = command.get_args().take(3).map(|word| word.to_string_lossy().into_owned()).collect();
    format!("`{program} {}`", words.join(" "))
}
