use std::ffi::OsStr;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use crate::foundation::step::{Refused, Step};

pub fn run<S: AsRef<OsStr>>(
    program: &str,
    dir: Option<&Path>,
    args: &[S],
) -> std::io::Result<Output> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.output()
}

pub async fn run_killing_on_drop(program: &str, args: &[String]) -> std::io::Result<Output> {
    tokio::process::Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
}

pub fn status_within(command: &mut Command, limit: Duration) -> std::io::Result<Option<ExitStatus>> {
    child_within(&mut command.spawn()?, limit)
}

/// Waits up to `limit` for an already spawned child; `None` means it was still running and has been killed and reaped.
pub fn child_within(child: &mut Child, limit: Duration) -> std::io::Result<Option<ExitStatus>> {
    child_within_with(child, limit, Child::try_wait)
}

fn child_within_with(child: &mut Child, limit: Duration, poll: fn(&mut Child) -> std::io::Result<Option<ExitStatus>>) -> std::io::Result<Option<ExitStatus>> {
    let started = Instant::now();
    loop {
        match poll(child) {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            child.wait()?;
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn run_within(program: &str, args: &[&str], limit: Duration) -> Option<Output> {
    run_with_input_within(program, args, None, limit)
}

pub fn run_with_input_within(
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
    limit: Duration,
) -> Option<Output> {
    let mut command = Command::new(program);
    command.args(args);
    run_command_within(command, input, limit)
}

pub fn run_command_within(command: Command, input: Option<&[u8]>, limit: Duration) -> Option<Output> {
    try_run_command_within(command, input, limit).ok().flatten()
}

pub fn try_run_command_within(command: Command, input: Option<&[u8]>, limit: Duration) -> std::io::Result<Option<Output>> {
    run_prepared(command, input, limit, false, false, false).map(Result::ok)
}

pub fn try_run_command_traced_within(command: Command, input: Option<&[u8]>, limit: Duration) -> std::io::Result<Result<Output, String>> {
    run_prepared(command, input, limit, false, true, false)
}

pub fn run_command_or_leave(mut command: Command, input: &[u8], limit: Duration) -> std::io::Result<Option<Output>> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    run_prepared(command, Some(input), limit, true, false, true).map(Result::ok)
}

pub fn run_group_within(mut command: Command, limit: Duration) -> Option<Output> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    run_prepared(command, None, limit, true, false, false).ok().and_then(Result::ok)
}

pub fn try_run_group_traced_within(mut command: Command, limit: Duration) -> std::io::Result<Result<Output, String>> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    run_prepared(command, None, limit, true, true, false)
}

pub enum Ended {
    Finished(Output),
    Stalled(String),
    Stopped,
}

/// Runs `command` in a process group of its own for up to `limit`, killing the group when `stop` answers true; `stop` is asked about twice a second.
pub fn try_run_group_until(mut command: Command, limit: Duration, stop: &mut dyn FnMut() -> bool) -> std::io::Result<Ended> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    run_until(command, None, limit, true, true, false, stop)
}

fn run_prepared(command: Command, input: Option<&[u8]>, limit: Duration, group: bool, traced: bool, leave: bool) -> std::io::Result<Result<Output, String>> {
    run_until(command, input, limit, group, traced, leave, &mut || false).map(|ended| match ended {
        Ended::Finished(output) => Ok(output),
        Ended::Stalled(stalled) => Err(stalled),
        Ended::Stopped => Err(String::new()),
    })
}

const STOP_POLL: Duration = Duration::from_millis(500);

fn run_until(mut command: Command, input: Option<&[u8]>, limit: Duration, group: bool, traced: bool, leave: bool, stop: &mut dyn FnMut() -> bool) -> std::io::Result<Ended> {
    let stdin = match input {
        Some(input) => {
            let mut file = tempfile::tempfile()?;
            file.write_all(input)?;
            file.rewind()?;
            Stdio::from(file)
        }
        None => Stdio::null(),
    };
    let output = tempfile::tempfile()?;
    let errors = tempfile::tempfile()?;
    let mut child = command.stdin(stdin).stdout(output.try_clone()?).stderr(errors.try_clone()?).spawn()?;
    let started = Instant::now();
    let mut asked = started;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            if group {
                kill(&mut child, group);
            }
            break status;
        }
        if started.elapsed() >= limit {
            if leave {
                return Ok(Ended::Stalled(String::new()));
            }
            let stalled = if traced { tree(child.id()) } else { String::new() };
            kill(&mut child, group);
            let _ = child.wait();
            return Ok(Ended::Stalled(stalled));
        }
        if asked.elapsed() >= STOP_POLL {
            asked = Instant::now();
            if stop() {
                kill(&mut child, group);
                let _ = child.wait();
                return Ok(Ended::Stopped);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut output = output;
    output.rewind()?;
    let mut stdout = Vec::new();
    output.read_to_end(&mut stdout)?;
    let mut errors = errors;
    errors.rewind()?;
    let mut stderr = Vec::new();
    errors.read_to_end(&mut stderr)?;
    Ok(Ended::Finished(Output { status, stdout, stderr }))
}

fn tree(pid: u32) -> String {
    let mut lines = Vec::new();
    describe(pid, 0, &mut lines);
    lines.join("; ")
}

fn describe(pid: u32, depth: usize, lines: &mut Vec<String>) {
    let proc = Path::new("/proc").join(pid.to_string());
    let stat = std::fs::read_to_string(proc.join("stat")).unwrap_or_default();
    let state = stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().next()).unwrap_or("?");
    let wchan = std::fs::read_to_string(proc.join("wchan")).ok().filter(|wchan| !wchan.is_empty() && wchan != "0").unwrap_or_else(|| "-".to_owned());
    let cmdline = std::fs::read(proc.join("cmdline")).unwrap_or_default();
    let cmdline: String = String::from_utf8_lossy(&cmdline).replace('\0', " ").trim_end().chars().take(160).collect();
    lines.push(format!("{}pid {pid} state {state} wchan {wchan}: {cmdline}", "> ".repeat(depth)));
    let children = std::fs::read_dir(proc.join("task")).into_iter().flatten().flatten().filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok());
    for child in children.collect::<Vec<_>>().join(" ").split_whitespace().filter_map(|child| child.parse().ok()) {
        describe(child, depth + 1, lines);
    }
}

fn kill(child: &mut std::process::Child, group: bool) {
    let leader = i32::try_from(child.id()).ok().and_then(rustix::process::Pid::from_raw);
    match leader {
        Some(pid) if group => {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::Kill);
        }
        _ => {
            let _ = child.kill();
        }
    }
}

pub struct Bounded {
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub overflowed: bool,
}

pub fn run_bounded<S: AsRef<OsStr>>(program: &str, dir: Option<&Path>, args: &[S], limit: u64) -> std::io::Result<Bounded> {
    let mut errors = tempfile::tempfile()?;
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(errors.try_clone()?);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command.spawn()?;
    let mut stdout = Vec::new();
    let read = match child.stdout.take() {
        Some(pipe) => pipe.take(limit.saturating_add(1)).read_to_end(&mut stdout).map(|_| ()),
        None => Ok(()),
    };
    let status = child.wait()?;
    read?;
    let mut stderr = Vec::new();
    errors.rewind()?;
    errors.read_to_end(&mut stderr)?;
    let overflowed = stdout.len() as u64 > limit;
    stdout.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    Ok(Bounded { status, stdout, stderr, overflowed })
}

pub fn exit_code(status: ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(1),
        (None, None) => 1,
    }
}

pub fn exit_status(status: std::io::Result<ExitStatus>) -> Step {
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(Refused(exit_code(status))),
        Err(_) => Err(Refused(127)),
    }
}

pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            let candidate = dir.join(program);
            candidate.is_file() && std::fs::metadata(&candidate).is_ok_and(|meta| std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0)
        })
    })
}

#[cfg(test)]
mod leave_tests {
    use super::*;

    #[test]
    fn a_hook_stops_waiting_at_its_limit_and_the_command_finishes_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, done, finished) = (dir.path().join("gate"), dir.path().join("done"), dir.path().join("finished"));
        assert!(Command::new("mkfifo").arg(&finished).status().unwrap().success());
        let mut command = Command::new("sh");
        command.args(["-c", "i=0; while [ ! -e \"$0\" ] && [ $i -lt 6000 ]; do sleep 0.02; i=$((i+1)); done; touch \"$1\"; echo > \"$2\"", &gate.display().to_string(), &done.display().to_string(), &finished.display().to_string()]);
        assert!(run_command_or_leave(command, b"", Duration::from_millis(200)).unwrap().is_none());
        assert!(!done.exists());
        std::fs::write(&gate, "").unwrap();
        std::fs::read_to_string(&finished).unwrap();
        assert!(done.exists());
    }
}

#[cfg(test)]
mod within_tests {
    use super::*;

    fn fails(_: &mut Child) -> std::io::Result<Option<ExitStatus>> {
        Err(std::io::Error::from_raw_os_error(10))
    }

    #[test]
    fn a_poll_error_kills_and_reaps_the_child() {
        let mut child = Command::new("sh").args(["-c", "sleep 30"]).spawn().unwrap();
        let pid = child.id();
        assert!(child_within_with(&mut child, Duration::from_secs(30), fails).is_err());
        assert!(!Command::new("kill").args(["-0", &pid.to_string()]).status().unwrap().success());
    }
}
