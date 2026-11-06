//! The decision log: one JSON line per decision placement, admission and the lander make, under
//! `decisions/KIND/YYYY-MM-DD/HH.jsonl` (UTC) in north's state on the host that decided
//! (docs/rationale/strategy-kernel.md). Recording never changes a decision: a failed write warns
//! once per process and is counted in the next line's `dropped`.
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use north_strategy_types::{Envelope, Kind, Snapshot, VERSION};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::foundation::clock::Timestamp;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static WARNED: AtomicBool = AtomicBool::new(false);
static WRITTEN_SNAPSHOTS: Mutex<Option<(PathBuf, BTreeSet<String>)>> = Mutex::new(None);

/// The decision log's root: `NORTH_DECISION_LOG` when set, none when it is empty, else
/// `$XDG_STATE_HOME/north/decisions`.
pub fn root() -> Option<PathBuf> {
    if let Some(chosen) = std::env::var_os("NORTH_DECISION_LOG") {
        return (!chosen.is_empty()).then(|| PathBuf::from(chosen));
    }
    let state = std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    Some(state.join("north/decisions"))
}

/// The file a line decided at `at` goes to: `KIND/YYYY-MM-DD/HH.SUFFIX` in UTC.
pub fn hour_file(root: &Path, kind: Kind, at: f64, suffix: &str) -> PathBuf {
    let stamp = Timestamp::from_unix_seconds_f64(at).iso();
    root.join(kind.name()).join(&stamp[..10]).join(format!("{}.{suffix}", &stamp[11..13]))
}

/// The runtime that decides: `live:SHA` of the promoted runtime this process runs from, else `live:unpromoted`.
pub fn strategy() -> String {
    let executable = std::env::current_exe().unwrap_or_default();
    format!("live:{}", super::harness::promoted_runtime(&executable).unwrap_or("unpromoted"))
}

/// The sha256 of a file a case records instead of holding it whole.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The clock reading a decision is stamped with, in unix seconds.
pub fn now() -> f64 {
    Timestamp::now().unix_seconds_f64()
}

/// Runs `decide` and returns its answer with the microseconds it took, the cost each line records.
pub fn timed<T>(decide: impl FnOnce() -> T) -> (T, u64) {
    let started = Instant::now();
    let answer = decide();
    (answer, u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX))
}

/// One decision to append.
pub struct Entry<'a, C, D> {
    pub kind: Kind,
    pub at: f64,
    pub follows: Option<&'a str>,
    pub case: &'a C,
    pub decision: &'a D,
    pub decide_us: u64,
}

/// Appends the decision and returns its id, which a later call that continues it names in `follows`.
pub fn record<C: Serialize, D: Serialize>(entry: Entry<'_, C, D>) -> String {
    let host = super::host::name();
    let id = format!("{host}-{:.3}-{}-{}", entry.at, std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed));
    let Some(root) = root() else { return id };
    let envelope = Envelope {
        v: VERSION,
        kind: entry.kind,
        id: id.clone(),
        at: entry.at,
        host,
        strategy: strategy(),
        follows: entry.follows.map(str::to_owned),
        case: entry.case,
        decision: entry.decision,
        decide_us: entry.decide_us,
        dropped: DROPPED.swap(0, Ordering::Relaxed),
    };
    append(&hour_file(&root, entry.kind, entry.at, "jsonl"), &envelope, envelope.dropped);
    id
}

/// Appends the admission snapshot unless this process already wrote it, and returns its hash.
pub fn snapshot<S: Serialize>(at: f64, snapshot: &S) -> String {
    let body = serde_json::to_string(snapshot).unwrap_or_default();
    let hash: String = Sha256::digest(body.as_bytes()).iter().take(12).map(|byte| format!("{byte:02x}")).collect();
    let Some(root) = root() else { return hash };
    let path = hour_file(&root, Kind::Admission, at, "snapshots.jsonl");
    let mut written = WRITTEN_SNAPSHOTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (hour, hashes) = written.get_or_insert_with(|| (path.clone(), BTreeSet::new()));
    if *hour != path {
        (*hour, *hashes) = (path.clone(), BTreeSet::new());
    }
    if hashes.insert(hash.clone()) {
        append(&path, &Snapshot { v: VERSION, hash: hash.clone(), at, host: super::host::name(), snapshot }, 0);
    }
    hash
}

fn append(path: &Path, line: &impl Serialize, carried: u64) {
    let written = serde_json::to_vec(line).map_err(|error| error.to_string()).and_then(|mut bytes| {
        bytes.push(b'\n');
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|error| error.to_string())?;
        OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| file.write_all(&bytes)).map_err(|error| error.to_string())
    });
    if let Err(error) = written {
        DROPPED.fetch_add(carried + 1, Ordering::Relaxed);
        if !WARNED.swap(true, Ordering::Relaxed) {
            eprintln!("north: decision log: cannot append to {}: {error}; the decision stands", path.display());
        }
    }
}
