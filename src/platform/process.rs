use std::ffi::OsStr;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn run<S: AsRef<OsStr>>(
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

pub(crate) async fn run_killing_on_drop(program: &str, args: &[String]) -> std::io::Result<Output> {
    tokio::process::Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
}

pub(crate) fn run_within(program: &str, args: &[&str], limit: Duration) -> Option<Output> {
    run_with_input_within(program, args, None, limit)
}

pub(crate) fn run_with_input_within(
    program: &str,
    args: &[&str],
    input: Option<&[u8]>,
    limit: Duration,
) -> Option<Output> {
    let stdin = match input {
        Some(input) => {
            let mut file = tempfile::tempfile().ok()?;
            file.write_all(input).ok()?;
            file.rewind().ok()?;
            Stdio::from(file)
        }
        None => Stdio::null(),
    };
    let output = tempfile::tempfile().ok()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(stdin)
        .stdout(output.try_clone().ok()?)
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut output = output;
    output.rewind().ok()?;
    let mut stdout = Vec::new();
    output.read_to_end(&mut stdout).ok()?;
    Some(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

pub(crate) struct Bounded {
    pub(crate) status: std::process::ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) overflowed: bool,
}

pub(crate) fn run_bounded<S: AsRef<OsStr>>(program: &str, dir: Option<&Path>, args: &[S], limit: u64) -> std::io::Result<Bounded> {
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
