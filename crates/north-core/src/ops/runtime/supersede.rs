use super::failure::{Failure, failed};
use super::layout;
use rustix::process::{Pid, Signal, kill_process_group};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

pub(super) const SUPERSEDED: u8 = 76;
const CHECK_EVERY: Duration = Duration::from_secs(20);
const TERM_GRACE: Duration = Duration::from_secs(10);

pub(super) fn run(command: &mut Command, target: &str, previous: Option<Duration>, head: impl Fn() -> Option<String>) -> Result<ExitStatus, Failure> {
    let started = Instant::now();
    let mut child = command.process_group(0).spawn()?;
    let every = std::env::var("NORTH_RUNTIME_WATCH_SECONDS")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map_or(CHECK_EVERY, Duration::from_secs);
    let mut next = Instant::now() + every;
    let mut finishing = false;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if !finishing && Instant::now() >= next {
            next = Instant::now() + every;
            if head().is_some_and(|head| head != target) {
                let short = layout::short(target);
                let Some(previous) = previous.filter(|previous| past_half(started.elapsed(), *previous)) else {
                    stop(&mut child);
                    return Err(failed(SUPERSEDED, format!("origin/main moved past {short}; abandoned its build")));
                };
                println!(
                    "north runtime: origin/main moved past {short}, but its build has run {}s of the last build's {}s; finishing it before building the new origin/main",
                    started.elapsed().as_secs(),
                    previous.as_secs()
                );
                finishing = true;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn past_half(elapsed: Duration, previous: Duration) -> bool {
    elapsed * 2 > previous
}

fn stop(child: &mut Child) {
    let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    if let Some(group) = group {
        let _ = kill_process_group(group, Signal::Term);
    }
    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline {
        if child.try_wait().is_ok_and(|status| status.is_some()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(group) = group {
        let _ = kill_process_group(group, Signal::Kill);
    }
    let _ = child.wait();
}
