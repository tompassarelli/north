use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::NorthResult;

use super::model::SessionRecord;

const RECORD: &str = "session.json";
pub const ARCHIVE: &str = ".archive";

static WRITES: AtomicU64 = AtomicU64::new(0);

pub fn read_record(session_dir: &Path) -> NorthResult<SessionRecord> {
    Ok(serde_json::from_str(&fs::read_to_string(session_dir.join(RECORD))?)?)
}

pub fn write_record(session_dir: &Path, record: &SessionRecord) -> NorthResult<()> {
    let temporary = session_dir.join(format!("{RECORD}.{}-{}.tmp", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
    fs::write(&temporary, serde_json::to_vec_pretty(record)?)?;
    fs::rename(temporary, session_dir.join(RECORD))?;
    Ok(())
}

pub fn find_session(root: &Path, host_dir: &Path, conversation: &str) -> Option<SessionRecord> {
    let listed = |directory: PathBuf| fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path());
    listed(root.to_path_buf())
        .chain(listed(root.join(ARCHIVE)))
        .filter_map(|session_dir| read_record(&session_dir).ok())
        .find(|record| record.host_dir == host_dir && record.conversation.as_deref() == Some(conversation))
}
