//! The caller's side of a pull: it decides whose turn it is, runs the ledger host's command over ssh, and
//! keeps why the last pull failed in the snapshot's tried marker until a pull succeeds.
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use super::FIRST_PULL_RETRY;
use super::pages::{hash_pages, invalid, manifest_bytes};
use super::receive::{publish, receive};
use super::sidecar::suffixed;
use crate::platform::process::child_within;

/// Whether this caller pulls now, which it does once the snapshot and its last try are older than `within` (always for zero); it marks its try first, so callers racing it do not pull again.
pub fn take_turn(snapshot: &Path, within: Duration) -> io::Result<bool> {
    let within = if snapshot.exists() { within } else { within.min(FIRST_PULL_RETRY) };
    let tried = suffixed(snapshot, ".tried");
    let newest = [snapshot, tried.as_path()].iter().filter_map(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok()).max();
    if !within.is_zero() && newest.is_some_and(|modified| SystemTime::now().duration_since(modified).map_or(true, |age| age < within)) {
        return Ok(false);
    }
    if let Some(parent) = snapshot.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(&tried)?.set_modified(SystemTime::now())?;
    Ok(true)
}

pub fn refresh(ssh: &[OsString], snapshot: &Path, (within, recent): (Duration, bool), wait: Duration) -> io::Result<bool> {
    if !take_turn(snapshot, within)? {
        return Ok(false);
    }
    let pulling = OpenOptions::new().create(true).append(true).open(suffixed(snapshot, ".tried"))?;
    match pulling.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Ok(false),
        Err(fs::TryLockError::Error(error)) => return Err(error),
    }
    let pulled = pull(ssh, snapshot, wait, recent);
    record(&pulling, pulled.as_ref().err())?;
    pulled.map(|()| true)
}

/// Why the last pull into `snapshot` failed, kept in its `.tried` marker until a pull succeeds.
pub fn last_failure(snapshot: &Path) -> Option<String> {
    fs::read_to_string(suffixed(snapshot, ".tried")).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
}

fn record(mut tried: &File, failure: Option<&io::Error>) -> io::Result<()> {
    tried.set_len(0)?;
    failure.map_or(Ok(()), |error| writeln!(tried, "{error}"))
}

/// Pulls the ledger image into `snapshot`; `recent` accepts one the ledger host built within `REUSE_SECONDS`, for a caller that reads a snapshot it tolerates being aged, not one that just wrote.
pub fn pull(ssh: &[OsString], snapshot: &Path, wait: Duration, recent: bool) -> io::Result<()> {
    let partial = suffixed(snapshot, ".partial");
    let fresh = !snapshot.exists();
    let old = File::open(snapshot).or_else(|_| File::open(&partial)).ok();
    let (size, hashes) = old.as_ref().map(hash_pages).unwrap_or((0, Vec::new()));
    let directory = snapshot.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let mut child = Command::new(&ssh[0]).args(&ssh[1..])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let manifest = manifest_bytes(size, &hashes, recent);
    let mut stdin = child.stdin.take().ok_or_else(|| invalid("ssh has no stdin"))?;
    std::thread::spawn(move || {
        let _ = stdin.write_all(&manifest);
    });
    let stdout = child.stdout.take().ok_or_else(|| invalid("ssh has no stdout"))?;
    let keep = fresh.then(|| partial.clone());
    let receiver = std::thread::spawn(move || receive(stdout, &directory, keep, old, size, hashes));
    let mut stderr = child.stderr.take().ok_or_else(|| invalid("ssh has no stderr"))?;
    let complaint = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text.lines().rev().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
    });
    let status = child_within(&mut child, wait)?.ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "snapshot pull timed out"))?;
    let built = receiver.join().map_err(|_| invalid("snapshot receiver panicked"))?;
    if !status.success() {
        let said = complaint.join().ok().flatten().map_or_else(String::new, |line| format!(": {line}"));
        return Err(io::Error::other(format!("ssh exited with {status}{said}")));
    }
    let file = built?;
    file.sync_all()?;
    publish(&file, snapshot)?;
    let _ = fs::remove_file(&partial);
    Ok(())
}
