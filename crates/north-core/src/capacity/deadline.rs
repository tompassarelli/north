use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::Value;
use tokio::process::Command;
use tokio::signal::unix::{Signal as Incoming, SignalKind, signal};

const USAGE: &str = "usage: north capacity deadline LEASE_FILE SECONDS -- COMMAND...";
const POLL: Duration = Duration::from_millis(250);
const KILL_AFTER: Duration = Duration::from_secs(5);
const TIMED_OUT: u8 = 124;
const KILLED: u8 = 137;
const ANCESTRY_LIMIT: usize = 256;

pub fn run(arguments: &[String]) -> u8 {
    let [lease, seconds, separator, program, rest @ ..] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(timeout) = seconds.parse().ok().filter(|_| separator == "--").map(Duration::from_secs) else {
        eprintln!("{USAGE}");
        return 2;
    };
    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime.block_on(guard(Clock::start(Path::new(lease), timeout), program, rest)),
        Err(error) => {
            eprintln!("north capacity deadline: {error}");
            1
        }
    }
}

async fn guard(mut clock: Clock<'_>, program: &str, rest: &[String]) -> u8 {
    let mut child = match Command::new(program).args(rest).process_group(0).spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("north capacity deadline: {program}: {error}");
            return 127;
        }
    };
    let group = child.id().and_then(|pid| Pid::from_raw(pid as i32));
    let send = |signal: Signal| group.map(|group| kill_process_group(group, signal));
    let mut term = signal(SignalKind::terminate()).ok();
    let mut interrupt = signal(SignalKind::interrupt()).ok();
    let mut hangup = signal(SignalKind::hangup()).ok();
    loop {
        tokio::select! {
            status = child.wait() => return status.map_or(1, exit_code),
            () = next(&mut term) => drop(send(Signal::Term)),
            () = next(&mut interrupt) => drop(send(Signal::Int)),
            () = next(&mut hangup) => drop(send(Signal::Hup)),
            () = tokio::time::sleep(POLL) => {
                if clock.expired() {
                    break;
                }
            }
        }
    }
    let _ = send(Signal::Term);
    if tokio::time::timeout(KILL_AFTER, child.wait()).await.is_ok() {
        return TIMED_OUT;
    }
    let _ = send(Signal::Kill);
    let _ = child.wait().await;
    KILLED
}

async fn next(stream: &mut Option<Incoming>) {
    match stream {
        Some(stream) => {
            stream.recv().await;
        }
        None => std::future::pending().await,
    }
}

struct Clock<'a> {
    lease: &'a Path,
    timeout: Duration,
    started: Instant,
    first: Option<Expiry>,
    paused: Duration,
    queued: Duration,
    queued_since: Option<Instant>,
}

impl<'a> Clock<'a> {
    fn start(lease: &'a Path, timeout: Duration) -> Self {
        Self { lease, timeout, started: Instant::now(), first: Expiry::read(lease), paused: Duration::ZERO, queued: Duration::ZERO, queued_since: None }
    }

    /// Time this command's processes spent queued for a capacity lease, which delays the deadline instead of spending it.
    fn waiting(&mut self) -> Duration {
        let now = Instant::now();
        match (self.queued_since, queued_beneath(self.lease)) {
            (None, true) => self.queued_since = Some(now),
            (Some(since), false) => {
                self.queued += now - since;
                self.queued_since = None;
            }
            _ => {}
        }
        self.queued + self.queued_since.map_or(Duration::ZERO, |since| now - since)
    }

    fn expired(&mut self) -> bool {
        let now = Expiry::read(self.lease);
        if let (Some(first), Some(now)) = (self.first, now) {
            self.paused = Duration::from_millis(now.expires_at.saturating_sub(first.expires_at));
        }
        let waiting = self.waiting();
        now.is_none_or(|now| !now.frozen) && self.started.elapsed() >= self.timeout + self.paused + waiting
    }
}

/// Whether a capacity queue ticket beside `lease` belongs to a process descended from this one.
fn queued_beneath(lease: &Path) -> bool {
    let Some(queue) = lease.parent().and_then(Path::parent).map(|root| root.join("queue")) else { return false };
    let Ok(tickets) = fs::read_dir(queue) else { return false };
    let me = std::process::id();
    tickets.flatten().map(|ticket| ticket.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).any(|path| wrapper(&path).is_some_and(|pid| descends(pid, me)))
}

fn wrapper(ticket: &Path) -> Option<u32> {
    serde_json::from_slice::<Value>(&fs::read(ticket).ok()?).ok()?["wrapperPid"].as_u64().and_then(|pid| u32::try_from(pid).ok())
}

fn descends(mut pid: u32, ancestor: u32) -> bool {
    for _ in 0..ANCESTRY_LIMIT {
        if pid == ancestor {
            return true;
        }
        let Some(parent) = parent_of(pid).filter(|parent| *parent != 0) else { return false };
        pid = parent;
    }
    false
}

fn parent_of(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split(' ').nth(1)?.parse().ok()
}

#[derive(Clone, Copy)]
struct Expiry {
    expires_at: u64,
    frozen: bool,
}

impl Expiry {
    fn read(lease: &Path) -> Option<Self> {
        let lease = serde_json::from_slice::<Value>(&fs::read(lease).ok()?).ok()?;
        Some(Self { expires_at: lease["expiresAt"].as_u64()?, frozen: !lease["frozenAt"].is_null() })
    }
}

fn exit_code(status: ExitStatus) -> u8 {
    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).and_then(|code| u8::try_from(code).ok()).unwrap_or(1)
}
