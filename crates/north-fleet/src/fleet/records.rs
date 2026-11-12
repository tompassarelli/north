use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use north_session_host::session_lifecycle::Phases;
use serde_json::Value;

use crate::error::NorthResult;

use super::model::SessionRecord;

const RECORD: &str = "session.json";
pub use north_session_host::session_lifecycle::ARCHIVE;
pub const PLACED: &str = ".placed";
/// The note a retired session's archive keeps: why, when, and its last assistant text.
pub const RETIRED_NOTE: &str = "retired.json";
/// Marks a session whose claims the exited sweep cleared, so later sweeps skip it until it starts again.
pub const CLAIMS_RELEASED: &str = "claims.released";
/// The fleet-wide log of the background claims sweep, which its one running sweep holds locked.
pub const CLAIMS_SWEEP_LOG: &str = ".claims-sweep.log";

static WRITES: AtomicU64 = AtomicU64::new(0);

pub fn record_file(session_dir: &Path) -> PathBuf {
    session_dir.join(RECORD)
}

pub fn read_record(session_dir: &Path) -> NorthResult<SessionRecord> {
    read_phased(Phases::beside(session_dir).as_mut(), session_dir)
}

/// A session record with the phase this host's lifecycle store holds for its launch, unread when the store is.
pub fn read_phased(phases: Option<&mut Phases>, session_dir: &Path) -> NorthResult<SessionRecord> {
    let legacy: Value = serde_json::from_str(&fs::read_to_string(session_dir.join(RECORD))?)?;
    let mut record: SessionRecord = serde_json::from_value(legacy.clone())?;
    record.phase = phases.and_then(|phases| phases.read(record.launch(), &legacy));
    Ok(record)
}

pub fn write_record(session_dir: &Path, record: &SessionRecord) -> NorthResult<()> {
    let temporary = session_dir.join(format!("{RECORD}.{}-{}.tmp", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
    fs::write(&temporary, serde_json::to_vec_pretty(record)?)?;
    fs::rename(temporary, session_dir.join(RECORD))?;
    Ok(())
}

/// Every session record in `directories` of the fleet at `root`, phased from one open lifecycle store.
fn records_in<const N: usize>(root: &Path, directories: [PathBuf; N]) -> impl Iterator<Item = SessionRecord> {
    let mut phases = Phases::reading(root);
    directories.into_iter().flat_map(|directory| fs::read_dir(directory).into_iter().flatten().flatten()).filter_map(move |entry| read_phased(phases.as_mut(), &entry.path()).ok())
}

/// A session the fleet at some root has a record of, with the record file that names it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedSession {
    pub name: String,
    pub provider: String,
    pub file: PathBuf,
    pub ended: bool,
}

/// Every session of the fleet at `root`, current or archived, whether it has ended: an archived one has, a current one has when its lifecycle phase says so.
pub fn recorded_sessions(root: &Path) -> Vec<RecordedSession> {
    let mut phases = Phases::reading(root);
    let mut sessions = Vec::new();
    for (directory, archived) in [(root.to_path_buf(), false), (root.join(ARCHIVE), true)] {
        for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
            let Ok(record) = read_phased(phases.as_mut(), &entry.path()) else { continue };
            sessions.push(RecordedSession { ended: archived || record.ended(), file: record_file(&entry.path()), provider: record.provider.label().to_owned(), name: record.name });
        }
    }
    sessions
}

pub fn find_session(root: &Path, host_dir: &Path, conversation: &str) -> Option<SessionRecord> {
    records_in(root, [root.to_path_buf(), root.join(ARCHIVE)]).find(|record| record.host_dir == host_dir && record.conversation.as_deref() == Some(conversation))
}

pub fn conversation_record(root: &Path, conversation: &str) -> Option<SessionRecord> {
    records_in(root, [root.to_path_buf(), root.join(ARCHIVE)]).find(|record| record.conversation.as_deref() == Some(conversation))
}

pub fn named_record_before(root: &Path, name: &str, before_ms: u64) -> Option<SessionRecord> {
    records_in(root, [root.to_path_buf(), root.join(ARCHIVE)]).filter(|record| record.name == name && record.created_ms <= before_ms).max_by_key(|record| record.created_ms)
}

pub fn past_records(root: &Path) -> Vec<SessionRecord> {
    let placed = fs::read_dir(root.join(PLACED)).into_iter().flatten().flatten().filter_map(|entry| read_phased(None, &entry.path()).ok());
    let mut records: Vec<SessionRecord> = records_in(root, [root.join(ARCHIVE)]).chain(placed).collect();
    records.sort_by_key(|record| record.created_ms);
    records
}

pub fn host_sessions(root: &Path, host_dir: &Path) -> Vec<SessionRecord> {
    records_in(root, [root.to_path_buf()]).filter(|record| record.host_dir == host_dir).collect()
}

pub fn parents_live(root: &Path, name: &str) -> bool {
    records_in(root, [root.to_path_buf()]).any(|record| record.parent.as_deref() == Some(name) && !record.ended())
}

/// What a retired session left behind: its last assistant text and when it was retired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetiredReport {
    pub text: String,
    pub at_ms: u64,
}

/// The newest retirement note of the session `name` archived under `root`.
pub fn retired_report(root: &Path, name: &str) -> Option<RetiredReport> {
    let prefix = format!("{name}-");
    fs::read_dir(root.join(ARCHIVE))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().strip_prefix(&prefix).is_some_and(|stamp| !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit())))
        .filter_map(|entry| serde_json::from_slice::<serde_json::Value>(&fs::read(entry.path().join(RETIRED_NOTE)).ok()?).ok())
        .filter_map(|note| Some(RetiredReport { text: note["last"].as_str()?.to_owned(), at_ms: note["at_ms"].as_u64()? }))
        .max_by_key(|report| report.at_ms)
}

pub fn recorded_names(root: &Path) -> BTreeSet<String> {
    let current = records_in(root, [root.to_path_buf()]);
    current.chain(past_records(root)).map(|record| record.name).collect()
}

