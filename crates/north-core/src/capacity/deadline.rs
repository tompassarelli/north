use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::Value;
use tokio::process::Command;
use tokio::signal::unix::{Signal as Incoming, SignalKind, signal};

const USAGE: &str = "usage: north capacity deadline LEASE_FILE|- SECONDS -- COMMAND...";
const POLL: Duration = Duration::from_millis(250);
const KILL_AFTER: Duration = Duration::from_secs(5);
const TIMED_OUT: u8 = 124;
const KILLED: u8 = 137;
const ANCESTRY_LIMIT: usize = 256;
const DRIVEN_CLOCK: &str = "NORTH_CAPACITY_DEADLINE_CLOCK";

pub fn run(arguments: &[String]) -> u8 {
    let [lease, seconds, separator, program, rest @ ..] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(timeout) = seconds.parse().ok().filter(|_| separator == "--").map(Duration::from_secs) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let lease = match Lease::named(lease) {
        Ok(lease) => lease,
        Err(why) => {
            eprintln!("north capacity deadline: {why}");
            return 2;
        }
    };
    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime.block_on(guard(Clock::start(lease, timeout), program, rest)),
        Err(error) => {
            eprintln!("north capacity deadline: {error}");
            1
        }
    }
}

async fn guard(mut clock: Clock, program: &str, rest: &[String]) -> u8 {
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
    if let Some(group) = group {
        report(program, rest, clock.timeout, group.as_raw_nonzero().get());
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

struct Lease {
    file: Option<PathBuf>,
    root: PathBuf,
}

impl Lease {
    fn named(argument: &str) -> Result<Self, String> {
        if argument == "-" {
            return Ok(Self { file: None, root: super::host::runtime_root() });
        }
        let file = PathBuf::from(argument);
        let root = file.parent().and_then(Path::parent).map(Path::to_path_buf);
        match (file.file_stem().filter(|stem| !stem.is_empty()), root) {
            (Some(_), Some(root)) => Ok(Self { file: Some(file), root }),
            _ => Err(format!("{argument:?} names no lease file; pass - for a command that holds no lease")),
        }
    }
}

struct Clock {
    lease: Lease,
    timeout: Duration,
    started: Instant,
    driven: Option<PathBuf>,
    first: Option<Expiry>,
    paused: Duration,
    queued: Duration,
    queued_since: Option<Duration>,
}

impl Drop for Clock {
    fn drop(&mut self) {
        if let Some(directory) = &self.driven {
            let _ = fs::remove_file(directory.join(format!("seen.{}", std::process::id())));
        }
    }
}

impl Clock {
    fn start(lease: Lease, timeout: Duration) -> Self {
        let driven = std::env::var_os(DRIVEN_CLOCK).filter(|directory| !directory.is_empty()).map(PathBuf::from);
        let first = lease.file.as_deref().and_then(Expiry::read);
        Self { lease, timeout, started: Instant::now(), driven, first, paused: Duration::ZERO, queued: Duration::ZERO, queued_since: None }
    }

    fn elapsed(&self) -> Duration {
        match &self.driven {
            None => self.started.elapsed(),
            Some(directory) => fs::read_to_string(directory.join("now")).ok().and_then(|text| text.trim().parse().ok()).map_or(Duration::ZERO, Duration::from_millis),
        }
    }

    fn acknowledge(&self, now: Duration) {
        if let Some(directory) = &self.driven {
            let _ = fs::write(directory.join(format!("seen.{}", std::process::id())), now.as_millis().to_string());
        }
    }

    fn waiting(&mut self, now: Duration) -> Duration {
        let held = queued_beneath(&self.lease.root) || (self.lease.file.is_none() && frozen_beneath(&self.lease.root));
        match (self.queued_since, held) {
            (None, true) => self.queued_since = Some(now),
            (Some(since), false) => {
                self.queued += now.saturating_sub(since);
                self.queued_since = None;
            }
            _ => {}
        }
        self.queued + self.queued_since.map_or(Duration::ZERO, |since| now.saturating_sub(since))
    }

    fn expired(&mut self) -> bool {
        let elapsed = self.elapsed();
        let lease = self.lease.file.as_deref().and_then(Expiry::read);
        if let (Some(first), Some(lease)) = (self.first, lease) {
            self.paused = Duration::from_millis(lease.expires_at.saturating_sub(first.expires_at));
        }
        let waiting = self.waiting(elapsed);
        let expired = lease.is_none_or(|lease| !lease.frozen) && elapsed >= self.timeout + self.paused + waiting;
        self.acknowledge(elapsed);
        expired
    }
}

fn queued_beneath(root: &Path) -> bool {
    beneath(&root.join("queue"), |_| true)
}

fn frozen_beneath(root: &Path) -> bool {
    beneath(&root.join("leases"), |record| !record["frozenAt"].is_null())
}

fn beneath(directory: &Path, matches: impl Fn(&Value) -> bool) -> bool {
    let Ok(records) = fs::read_dir(directory) else { return false };
    let me = std::process::id();
    records.flatten().map(|record| record.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).filter_map(|path| serde_json::from_slice::<Value>(&fs::read(path).ok()?).ok()).any(|record| matches(&record) && wrapper(&record).is_some_and(|pid| descends(pid, me)))
}

fn wrapper(record: &Value) -> Option<u32> {
    record["wrapperPid"].as_u64().and_then(|pid| u32::try_from(pid).ok())
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

fn report(program: &str, rest: &[String], timeout: Duration, leader: i32) {
    let command = std::iter::once(program).chain(rest.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
    eprintln!("north capacity deadline: {command} exceeded its {} s deadline; still running:", timeout.as_secs());
    eprint!("{}", super::process_tree::render(Path::new("/proc"), leader));
}
