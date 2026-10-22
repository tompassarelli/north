use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::process::{Pid, Signal, kill_process};
use serde_json::Value;

const LOCK_STALE: Duration = Duration::from_secs(2);
const LOCK_PATIENCE: Duration = Duration::from_secs(2);

pub(crate) fn pause(capacity: &Path, ids: &BTreeSet<String>) {
    rewrite(capacity, ids, |lease, now| {
        if !lease["frozenAt"].is_null() {
            return false;
        }
        lease["frozenAt"] = now.into();
        true
    });
}

pub(crate) fn resume(capacity: &Path, ids: &BTreeSet<String>) {
    rewrite(capacity, ids, |lease, now| {
        let Some(frozen_at) = lease.as_object_mut().and_then(|fields| fields.remove("frozenAt")).and_then(|at| at.as_u64()) else {
            return false;
        };
        if let Some(expires_at) = lease["expiresAt"].as_u64() {
            lease["expiresAt"] = (expires_at + now.saturating_sub(frozen_at)).into();
        }
        true
    });
}

fn rewrite(capacity: &Path, ids: &BTreeSet<String>, change: impl Fn(&mut Value, u64) -> bool) {
    if ids.is_empty() {
        return;
    }
    let Some(_lock) = Lock::take(capacity) else { return };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64);
    for path in ids.iter().map(|id| capacity.join("leases").join(format!("{id}.json"))) {
        let Some(mut lease) = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) else { continue };
        if change(&mut lease, now) {
            let _ = replace(&path, &format!("{lease}\n"));
        }
    }
}

fn replace(path: &Path, text: &str) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temporary)?.write_all(text.as_bytes())?;
    fs::rename(temporary, path)
}

struct Lock(PathBuf);

impl Lock {
    fn take(capacity: &Path) -> Option<Self> {
        let path = capacity.join("lock");
        let deadline = Instant::now() + LOCK_PATIENCE;
        while Instant::now() < deadline {
            match fs::create_dir(&path) {
                Ok(()) => return Some(Self(path)),
                Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => return None,
                Err(_) => {}
            }
            if fs::metadata(&path).and_then(|metadata| metadata.modified()).is_ok_and(|at| at.elapsed().is_ok_and(|age| age > LOCK_STALE)) {
                let _ = fs::remove_dir_all(&path);
                continue;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const USAGE: &str = "usage: north capacity deadline LEASE_FILE SECONDS -- COMMAND...";
const POLL: Duration = Duration::from_millis(250);
const KILL_AFTER: Duration = Duration::from_secs(5);

pub(crate) fn run(arguments: &[String]) -> u8 {
    let [lease, seconds, separator, program, rest @ ..] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    let Some(timeout) = seconds.parse().ok().filter(|_| separator == "--").map(Duration::from_secs) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut child = match Command::new(program).args(rest).spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("north capacity deadline: {program}: {error}");
            return 127;
        }
    };
    let lease = Path::new(lease);
    let started = Instant::now();
    let first = Expiry::read(lease);
    let mut paused = Duration::ZERO;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return exit_code(status);
        }
        let now = Expiry::read(lease);
        if let (Some(first), Some(now)) = (first, now) {
            paused = Duration::from_millis(now.expires_at.saturating_sub(first.expires_at));
        }
        if now.is_none_or(|now| !now.frozen) && started.elapsed() >= timeout + paused {
            break;
        }
        std::thread::sleep(POLL);
    }
    terminate(&mut child)
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

fn terminate(child: &mut Child) -> u8 {
    let pid = Pid::from_raw(child.id() as i32);
    let send = |signal: Signal| pid.map(|pid| kill_process(pid, signal));
    let _ = send(Signal::Term);
    let deadline = Instant::now() + KILL_AFTER;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return exit_code(status);
        }
        std::thread::sleep(POLL);
    }
    let _ = send(Signal::Kill);
    child.wait().map_or(137, exit_code)
}

fn exit_code(status: ExitStatus) -> u8 {
    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).and_then(|code| u8::try_from(code).ok()).unwrap_or(1)
}
