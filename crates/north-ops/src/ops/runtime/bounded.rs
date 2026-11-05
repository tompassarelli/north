use std::io::{Error, ErrorKind, Result};
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

pub(super) fn status(command: &mut Command, limit: Duration) -> Result<ExitStatus> {
    let mut child = command.spawn()?;
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
