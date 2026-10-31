use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::NorthResult;

use super::model::SessionRecord;

const RECORD: &str = "session.json";
pub const ARCHIVE: &str = ".archive";
pub const PLACED: &str = ".placed";

static WRITES: AtomicU64 = AtomicU64::new(0);

pub fn record_file(session_dir: &Path) -> PathBuf {
    session_dir.join(RECORD)
}

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

pub fn past_records(root: &Path) -> Vec<SessionRecord> {
    let mut records: Vec<SessionRecord> = [ARCHIVE, PLACED]
        .into_iter()
        .flat_map(|directory| fs::read_dir(root.join(directory)).into_iter().flatten().flatten())
        .filter_map(|entry| read_record(&entry.path()).ok())
        .collect();
    records.sort_by_key(|record| record.created_ms);
    records
}

pub fn host_sessions(root: &Path, host_dir: &Path) -> Vec<SessionRecord> {
    fs::read_dir(root).into_iter().flatten().flatten().filter_map(|entry| read_record(&entry.path()).ok()).filter(|record| record.host_dir == host_dir).collect()
}

pub fn parents_live(root: &Path, name: &str) -> bool {
    fs::read_dir(root).into_iter().flatten().flatten().filter_map(|entry| read_record(&entry.path()).ok()).any(|record| record.parent.as_deref() == Some(name) && !record.stopped)
}

pub fn recorded_names(root: &Path) -> BTreeSet<String> {
    let current = fs::read_dir(root).into_iter().flatten().flatten().filter_map(|entry| read_record(&entry.path()).ok());
    current.chain(past_records(root)).map(|record| record.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str) -> SessionRecord {
        serde_json::from_value(serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null})).unwrap()
    }

    #[test]
    fn recorded_names_cover_current_archived_and_placed_sessions_only() {
        let root = tempfile::tempdir().unwrap();
        for (directory, name) in [("live", "sc-live"), (".archive/gone-1", "sc-gone"), (".placed/abc", "sc-placed")] {
            let session_dir = root.path().join(directory);
            fs::create_dir_all(&session_dir).unwrap();
            write_record(&session_dir, &record(name)).unwrap();
        }
        let names: Vec<String> = recorded_names(root.path()).into_iter().collect();
        assert_eq!(names, ["sc-gone", "sc-live", "sc-placed"]);
    }
}
