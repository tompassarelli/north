use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::session_lifecycle::{ARCHIVE, Phases};
use serde_json::Value;

use crate::error::NorthResult;
use crate::session::SessionRecord;

const RECORD: &str = "session.json";

static WRITES: AtomicU64 = AtomicU64::new(0);

pub fn record_file(session_dir: &Path) -> PathBuf {
    session_dir.join(RECORD)
}

/// The session record in `session_dir`, phased from the lifecycle store only once the record is read, so a hook asking after a session with no record opens and migrates no store (north#1036).
pub fn read_record(session_dir: &Path) -> NorthResult<SessionRecord> {
    let text = fs::read_to_string(session_dir.join(RECORD))?;
    phased(Phases::beside(session_dir).as_mut(), &text)
}

/// A session record with the phase this host's lifecycle store holds for its launch, unread when the store is.
pub fn read_phased(phases: Option<&mut Phases>, session_dir: &Path) -> NorthResult<SessionRecord> {
    phased(phases, &fs::read_to_string(session_dir.join(RECORD))?)
}

fn phased(phases: Option<&mut Phases>, text: &str) -> NorthResult<SessionRecord> {
    let legacy: Value = serde_json::from_str(text)?;
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
pub fn records_in<const N: usize>(root: &Path, directories: [PathBuf; N]) -> impl Iterator<Item = SessionRecord> {
    let mut phases = Phases::reading(root);
    directories.into_iter().flat_map(|directory| fs::read_dir(directory).into_iter().flatten().flatten()).filter_map(move |entry| read_phased(phases.as_mut(), &entry.path()).ok())
}

/// A session the fleet at some root has a record of, with the record file that names it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedSession {
    pub name: String,
    pub provider: String,
    pub file: PathBuf,
    pub cwd: PathBuf,
    pub ended: bool,
}

/// Every session of the fleet at `root`, current or archived, whether it has ended: an archived one has, a current one has when its lifecycle phase says so.
pub fn recorded_sessions(root: &Path) -> Vec<RecordedSession> {
    let mut phases = Phases::reading(root);
    let mut sessions = Vec::new();
    for (directory, archived) in [(root.to_path_buf(), false), (root.join(ARCHIVE), true)] {
        for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
            let Ok(record) = read_phased(phases.as_mut(), &entry.path()) else { continue };
            sessions.push(RecordedSession { ended: archived || record.ended(), file: record_file(&entry.path()), cwd: record.cwd, provider: record.provider.label().to_owned(), name: record.name });
        }
    }
    sessions
}

pub fn find_session(root: &Path, host_dir: &Path, conversation: &str) -> Option<SessionRecord> {
    records_in(root, [root.to_path_buf(), root.join(ARCHIVE)]).find(|record| record.host_dir == host_dir && record.conversation.as_deref() == Some(conversation))
}
