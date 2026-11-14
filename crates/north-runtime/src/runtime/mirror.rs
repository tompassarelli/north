use super::bounded;
use super::failure::{Failure, failed};
use super::follow::own_scope;
use super::remote::OWN_RSH;
use north_offload::land_offload;
use crate::platform::host;
use crate::platform::records_mirror::{self, ACCEPTS, CHECKOUT_PREFIX, QUEUES, ROOT, STAMP, Stamp, TICK_JOURNAL};
use rustix::fs::{FlockOperation, flock};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOCK: &str = "north/records-mirror.lock";
const LAST: &str = "north/records-mirror.last";
const SENT: &str = "north/records-mirror-sent";
const CURSOR: &str = "north/fleet-tick.cursor";
const TICK_UNIT: &str = "fleet-tick.service";
const JOURNAL_FIRST: &str = "--since=-24h";
const JOURNAL_CAP_BYTES: u64 = 8 << 20;
const READ_JOURNAL: Duration = Duration::from_secs(5);
const COPY: Duration = Duration::from_secs(10 * 60);
const PRUNE: Duration = Duration::from_secs(60);
const RSYNC_PARTIAL: [i32; 2] = [23, 24];
const BATCH_BYTES: u64 = 1 << 30;
const BATCH_FILES: usize = 20_000;
const BANDWIDTH_KIB: u32 = 8192;
const PARTIAL_DIR: &str = ".rsync-partial";
const CARRY: Duration = Duration::from_secs(20);

type Signature = (u64, u128);
type Sent = BTreeMap<String, Signature>;

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    path: PathBuf,
    signature: Signature,
}

pub fn publish() -> Vec<String> {
    if !land_offload::settles_fleet_here() {
        return Vec::new();
    }
    let mut lines: Vec<String> = fs::read_to_string(records_mirror::state_home().join(LAST)).unwrap_or_default().lines().map(str::to_owned).collect();
    if let Err(error) = std::env::current_exe().and_then(|north| own_scope(north).args(["runtime", "mirror"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()) {
        lines.push(format!("records mirror: not started: {error}"));
    }
    lines
}

pub(super) fn run() -> Result<(), Failure> {
    if !land_offload::settles_fleet_here() {
        return Err(failed(1, "this host does not settle the fleet, so it holds no records to copy"));
    }
    let state = records_mirror::state_home();
    let lock = File::create(state.join(LOCK))?;
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        println!("north runtime mirror: the last copy is still running");
        return Ok(());
    }
    let destinations = land_offload::other_destinations();
    let (copied, failures) = publish_from(&state, &destinations)?;
    fs::write(state.join(LAST), failures.join("\n"))?;
    if !failures.is_empty() {
        return Err(failed(1, failures.join("; ")));
    }
    println!("north runtime mirror: copied {copied} records to {}", destinations.join(", "));
    Ok(())
}

/// Copies the accepts store `store` of the host that settles the fleet into the records mirror of every host it ships runtimes to, so a scan there counts an accept, or stops counting a withdrawn one, by the time the reviewer is told; why each host was not reached.
pub(crate) fn carry_accepts(store: &Path) -> Vec<String> {
    if !land_offload::settles_fleet_here() {
        return Vec::new();
    }
    let targets: Vec<String> = land_offload::other_destinations().iter().map(|destination| format!("{destination}:.local/state/{ROOT}/{ACCEPTS}/")).collect();
    carry(store, &targets)
}

fn carry(store: &Path, targets: &[String]) -> Vec<String> {
    let lock = match File::create(store.with_extension("lock")) {
        Ok(lock) => lock,
        Err(error) => return vec![format!("the carry lock beside {}: {error}", store.display())],
    };
    if let Err(error) = flock(&lock, FlockOperation::LockExclusive) {
        return vec![format!("the carry lock beside {}: {error}", store.display())];
    }
    let sends: Vec<_> = targets
        .iter()
        .map(|target| {
            let (store, to) = (store.to_owned(), target.clone());
            (target, std::thread::spawn(move || send_accepts(&store, &to)))
        })
        .collect();
    sends.into_iter().filter_map(|(target, send)| send.join().unwrap_or_else(|_| Err("the copy panicked".to_owned())).err().map(|why| format!("{target}: {why}"))).collect()
}

fn send_accepts(store: &Path, target: &str) -> Result<(), String> {
    let mut rsync = Command::new("rsync");
    rsync.args(["-r", "-t", "--delete", "--mkpath", "--timeout=10", "-e", OWN_RSH]).arg(format!("{}/", store.display())).arg(target).stdin(Stdio::null()).stdout(Stdio::null());
    let (status, said) = bounded::status_keeping_stderr(&mut rsync, CARRY, 4).map_err(|error| error.to_string())?;
    match status.success() {
        true => Ok(()),
        false => Err(format!("rsync {status}: {}", said.join(" | "))),
    }
}

fn publish_from(state: &Path, destinations: &[String]) -> Result<(usize, Vec<String>), Failure> {
    if let Err(why) = export_journal(state) {
        eprintln!("north runtime mirror: fleet tick journal not exported: {why}");
    }
    let stamp = Stamp { host: host::name(), state: state.to_owned(), at: records_mirror::now() };
    fs::write(state.join(STAMP), serde_json::to_vec(&stamp).map_err(|error| failed(1, error.to_string()))?)?;
    let mut paths = records_mirror::selected(state, SystemTime::now());
    paths.push(PathBuf::from(STAMP));
    let current: Vec<Entry> = paths.into_iter().filter_map(|path| signature(&state.join(&path)).map(|signature| Entry { path, signature })).collect();
    fs::create_dir_all(state.join(SENT))?;
    let failures = destinations.iter().filter_map(|destination| prune(state, destination).and_then(|()| copy(state, destination, &current)).err().map(|why| format!("records mirror: not copied to {destination}: {why}"))).collect();
    Ok((current.len(), failures))
}

fn prune(state: &Path, destination: &str) -> Result<(), String> {
    let empty = state.join(SENT).join("empty");
    for queue in fs::read_dir(state.join(QUEUES)).into_iter().flatten().flatten() {
        fs::create_dir_all(empty.join(QUEUES).join(queue.file_name())).map_err(|error| error.to_string())?;
    }
    let mut rsync = Command::new("rsync");
    rsync
        .args(["-r", "--delete", "--timeout=20", "-e", OWN_RSH])
        .arg(format!("--include=/{QUEUES}/*/{CHECKOUT_PREFIX}*/***"))
        .args(["--include=/north/".to_owned(), format!("--include=/{QUEUES}/"), format!("--include=/{QUEUES}/*/"), "--exclude=*".to_owned()])
        .arg(format!("{}/", empty.display()))
        .arg(format!("{destination}:.local/state/{ROOT}/"))
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let (status, said) = bounded::status_keeping_stderr(&mut rsync, PRUNE, 4).map_err(|error| error.to_string())?;
    match status.success() {
        true => Ok(()),
        false => Err(format!("rsync {status} while removing the checkouts the mirror no longer carries: {}", said.join(" | "))),
    }
}

fn signature(path: &Path) -> Option<Signature> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos()))
}

fn copy(state: &Path, destination: &str, current: &[Entry]) -> Result<(), String> {
    let sent = state.join(SENT).join(destination.replace('/', "_"));
    let list = sent.with_extension("list");
    let started = Instant::now();
    sync(&sent, current, |paths| {
        let listed: Vec<u8> = paths.iter().flat_map(|path| path.as_os_str().as_encoded_bytes().iter().copied().chain([0])).collect();
        fs::write(&list, &listed).map_err(|error| error.to_string())?;
        let mut rsync = Command::new("rsync");
        rsync
            .args(["-a", "--copy-dirlinks", "--delete-missing-args", "--from0", "--timeout=20", "--partial-dir", PARTIAL_DIR, "-e", OWN_RSH])
            .arg(format!("--bwlimit={BANDWIDTH_KIB}"))
            .arg(format!("--files-from={}", list.display()))
            .arg(format!("{}/", state.display()))
            .arg(format!("{destination}:.local/state/{ROOT}/"))
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        let (status, said) = bounded::status_keeping_stderr(&mut rsync, COPY.saturating_sub(started.elapsed()), 4).map_err(|error| error.to_string())?;
        match status.success() || status.code().is_some_and(|code| RSYNC_PARTIAL.contains(&code)) {
            true => Ok(()),
            false => Err(format!("rsync {status}: {}", said.join(" | "))),
        }
    })
}

fn sync(sent: &Path, current: &[Entry], mut send: impl FnMut(&[PathBuf]) -> Result<(), String>) -> Result<(), String> {
    let mut done: Sent = fs::read(sent).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let mut gone: Vec<PathBuf> = done.keys().filter(|path| !current.iter().any(|entry| entry.path.to_str() == Some(path.as_str()))).map(PathBuf::from).collect();
    gone.sort();
    let mut pending: Vec<&Entry> = current.iter().filter(|entry| done.get(entry.path.to_string_lossy().as_ref()) != Some(&entry.signature)).collect();
    pending.reverse();
    let mut batches: Vec<Vec<&Entry>> = Vec::new();
    while let Some(entry) = pending.pop() {
        let mut bytes = 0;
        let mut batch = Vec::new();
        let mut next = Some(entry);
        while let Some(entry) = next.take() {
            bytes += entry.signature.0;
            batch.push(entry);
            next = pending.pop_if(|_| bytes < BATCH_BYTES && batch.len() < BATCH_FILES);
        }
        batches.push(batch);
    }
    if batches.is_empty() && !gone.is_empty() {
        batches.push(Vec::new());
    }
    for (index, batch) in batches.iter().enumerate() {
        let removed = if index == 0 { std::mem::take(&mut gone) } else { Vec::new() };
        let listed: Vec<PathBuf> = batch.iter().map(|entry| entry.path.clone()).chain(removed.iter().cloned()).collect();
        send(&listed)?;
        for entry in batch {
            done.insert(entry.path.to_string_lossy().into_owned(), entry.signature);
        }
        for path in &removed {
            done.remove(path.to_string_lossy().as_ref());
        }
        fs::write(sent, serde_json::to_vec(&done).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    }
    Ok(())
}
fn export_journal(state: &Path) -> Result<(), String> {
    let cursor = state.join(CURSOR);
    let mut journalctl = Command::new("journalctl");
    journalctl.args(["--user", "--unit", TICK_UNIT, "--output=short-iso", "--no-pager", "--quiet"]).stdin(Stdio::null()).stderr(Stdio::null());
    let first = !cursor.is_file();
    match first {
        true => journalctl.args([JOURNAL_FIRST, "--show-cursor"]),
        false => journalctl.arg(format!("--cursor-file={}", cursor.display())),
    };
    let (status, said) = bounded::output(&mut journalctl, READ_JOURNAL).map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("journalctl {status}"));
    }
    let lines = match first {
        true => {
            let (lines, last) = said.trim_end().rsplit_once('\n').unwrap_or(("", said.trim_end()));
            let at = last.strip_prefix("-- cursor: ").ok_or("journalctl named no cursor")?;
            fs::write(&cursor, at).map_err(|error| error.to_string())?;
            if lines.is_empty() { String::new() } else { format!("{lines}\n") }
        }
        false => said,
    };
    let journal = state.join(TICK_JOURNAL);
    if fs::metadata(&journal).is_ok_and(|metadata| metadata.len() > JOURNAL_CAP_BYTES) {
        let text = fs::read(&journal).unwrap_or_default();
        let half = text.len() / 2;
        let start = text[half..].iter().position(|byte| *byte == b'\n').map_or(text.len(), |at| half + at + 1);
        fs::write(&journal, &text[start..]).map_err(|error| error.to_string())?;
    }
    OpenOptions::new().create(true).append(true).open(&journal).and_then(|mut file| file.write_all(lines.as_bytes())).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "mirror_tests.rs"]
mod tests;
