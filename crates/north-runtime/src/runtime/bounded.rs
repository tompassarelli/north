use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Error, ErrorKind, Result};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::platform::process::child_within;

const STOP_POLL: Duration = Duration::from_millis(100);

pub(super) fn status(command: &mut Command, limit: Duration) -> Result<ExitStatus> {
    wait(command.spawn()?, limit)
}

pub(super) fn status_keeping_stderr(command: &mut Command, limit: Duration, kept: usize) -> Result<(ExitStatus, Vec<String>)> {
    status_keeping_stderr_until(command, limit, kept, &|| false)
}

pub(super) fn status_keeping_stderr_until(command: &mut Command, limit: Duration, kept: usize, stop: &dyn Fn() -> bool) -> Result<(ExitStatus, Vec<String>)> {
    let started = Instant::now();
    status_keeping_stderr_by(command, limit, kept, stop, &|| started.elapsed())
}

fn status_keeping_stderr_by(command: &mut Command, limit: Duration, kept: usize, stop: &dyn Fn() -> bool, elapsed: &dyn Fn() -> Duration) -> Result<(ExitStatus, Vec<String>)> {
    let mut child = command.stderr(Stdio::piped()).spawn()?;
    let stderr = child.stderr.take().ok_or_else(|| Error::other("child stderr was not piped"))?;
    let reader = std::thread::spawn(move || {
        let mut tail = VecDeque::with_capacity(kept + 1);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("{line}");
            tail.push_back(line);
            if tail.len() > kept {
                tail.pop_front();
            }
        }
        Vec::from(tail)
    });
    let status = wait_until(child, limit, stop, elapsed)?;
    Ok((status, reader.join().unwrap_or_default()))
}

pub(super) fn output(command: &mut Command, limit: Duration) -> Result<(ExitStatus, String)> {
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    let mut stdout = child.stdout.take().ok_or_else(|| Error::other("child stdout was not piped"))?;
    let reader = std::thread::spawn(move || {
        let mut said = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut said);
        said
    });
    let status = wait(child, limit)?;
    Ok((status, reader.join().unwrap_or_default()))
}

fn wait_until(mut child: Child, limit: Duration, stop: &dyn Fn() -> bool, elapsed: &dyn Fn() -> Duration) -> Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let interrupted = stop();
        if interrupted || elapsed() >= limit {
            let _ = child.kill();
            child.wait()?;
            return Err(match interrupted {
                true => Error::new(ErrorKind::Interrupted, "stopped"),
                false => Error::new(ErrorKind::TimedOut, format!("timed out after {}s", limit.as_secs())),
            });
        }
        std::thread::sleep(STOP_POLL);
    }
}

fn wait(mut child: Child, limit: Duration) -> Result<ExitStatus> {
    child_within(&mut child, limit)?.ok_or_else(|| Error::new(ErrorKind::TimedOut, format!("timed out after {}s", limit.as_secs())))
}
