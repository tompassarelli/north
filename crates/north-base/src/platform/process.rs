use std::ffi::OsStr;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

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
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
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
    run_prepared(command, input, limit, false)
}

pub fn run_group_within(mut command: Command, limit: Duration) -> Option<Output> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    run_prepared(command, None, limit, true).ok().flatten()
}

fn run_prepared(mut command: Command, input: Option<&[u8]>, limit: Duration, group: bool) -> std::io::Result<Option<Output>> {
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
    let status = loop {
        if let Some(status) = child.try_wait()? {
            if group {
                kill(&mut child, group);
            }
            break status;
        }
        if started.elapsed() >= limit {
            kill(&mut child, group);
            let _ = child.wait();
            return Ok(None);
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
    Ok(Some(Output { status, stdout, stderr }))
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
