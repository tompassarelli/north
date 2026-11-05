use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Error, ErrorKind, Result};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub(super) fn status(command: &mut Command, limit: Duration) -> Result<ExitStatus> {
    wait(command.spawn()?, limit)
}

pub(super) fn status_keeping_stderr(command: &mut Command, limit: Duration, kept: usize) -> Result<(ExitStatus, Vec<String>)> {
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
    let status = wait(child, limit)?;
    Ok((status, reader.join().unwrap_or_default()))
}

fn wait(mut child: Child, limit: Duration) -> Result<ExitStatus> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err(Error::new(ErrorKind::TimedOut, format!("timed out after {}s", limit.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_past_its_limit_is_killed_and_reported_as_timed_out() {
        let started = Instant::now();
        let error = status(Command::new("sleep").arg("30"), Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
