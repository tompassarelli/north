use super::bounded;
use super::failure::{Failure, code, failed};
use std::io::ErrorKind;
use std::process::Command;
use std::time::Duration;

const REPORTED_LINES: usize = 12;

pub(super) const OWN_RSH: &str = "ssh -o ControlPath=none";

pub(super) fn ssh(destination: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4", "--", destination]);
    command
}

pub(super) fn limit(default: Duration) -> Duration {
    std::env::var("NORTH_RUNTIME_STEP_SECONDS")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map_or(default, Duration::from_secs)
}

pub(super) fn run(host: &str, step: &str, command: &mut Command, default: Duration) -> Result<(), Failure> {
    let limit = limit(default);
    let status = bounded::status(command, limit).map_err(|error| stopped(host, step, limit, error))?;
    match status.success() {
        true => Ok(()),
        false => Err(failed(code(status), format!("{step} on {host} failed: {status}"))),
    }
}

pub(super) fn output(host: &str, step: &str, command: &mut Command, default: Duration) -> Result<String, Failure> {
    let limit = limit(default);
    let (status, said) = bounded::output(command, limit).map_err(|error| stopped(host, step, limit, error))?;
    match status.success() {
        true => Ok(said),
        false => Err(failed(code(status), format!("{step} on {host} failed: {status}"))),
    }
}

pub(super) fn run_reporting(host: &str, step: &str, command: &mut Command, default: Duration) -> Result<(), Failure> {
    let limit = limit(default);
    let (status, said) = bounded::status_keeping_stderr(command, limit, REPORTED_LINES).map_err(|error| stopped(host, step, limit, error))?;
    match (status.success(), said.is_empty()) {
        (true, _) => Ok(()),
        (false, true) => Err(failed(code(status), format!("{step} on {host} failed: {status}"))),
        (false, false) => Err(failed(code(status), format!("{step} on {host} failed: {status}; it said: {}", said.join(" | ")))),
    }
}

fn stopped(host: &str, step: &str, limit: Duration, error: std::io::Error) -> Failure {
    match error.kind() {
        ErrorKind::TimedOut => failed(1, format!("{step} on {host} did not finish within {}s and was stopped", limit.as_secs())),
        _ => Failure::from(error),
    }
}
