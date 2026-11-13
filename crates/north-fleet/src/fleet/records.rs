use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::model::SessionRecord;

pub use north_fleet_store::session_files::{RecordedSession, find_session, read_phased, read_record, record_file, recorded_sessions, records_in, write_record};
pub use super::session_lifecycle::ARCHIVE;
pub use crate::platform::session_host_files::PLACED;
/// The note a retired session's archive keeps: why, when, and its last assistant text.
pub const RETIRED_NOTE: &str = "retired.json";
/// Marks a session whose claims the exited sweep cleared, so later sweeps skip it until it starts again.
pub const CLAIMS_RELEASED: &str = "claims.released";
/// The fleet-wide log of the background claims sweep, which its one running sweep holds locked.
pub const CLAIMS_SWEEP_LOG: &str = ".claims-sweep.log";

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

/// Whether the launcher recorded the session whose scope unit is `unit`, among the fleet at `root`, as launched `--priority urgent`; a unit no session names, or an unreadable record, is not urgent.
pub fn launched_urgent_in(root: &Path, unit: &str) -> bool {
    let named = |entry: &fs::DirEntry| fs::read_to_string(crate::platform::session_host_files::scope_path(&entry.path())).is_ok_and(|scope| scope.trim() == unit);
    let urgent = |entry: &fs::DirEntry| fs::read_to_string(record_file(&entry.path())).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).is_some_and(|record| record["urgent"] == Value::Bool(true));
    fs::read_dir(root).into_iter().flatten().flatten().any(|entry| named(&entry) && urgent(&entry))
}

pub fn held_urgent(root: &Path, home: &Path, unit: &str) -> Option<bool> {
    let conversation = fs::read_to_string(home.join(crate::platform::session_host_files::HELD).join(unit).join(crate::platform::session_host_files::BOUND_CONVERSATION)).ok()?.trim().to_owned();
    if conversation.is_empty() || !conversation.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') {
        return None;
    }
    let record = fs::read_to_string(record_file(&root.join(crate::platform::session_host_files::PLACED).join(conversation))).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    Some(record.is_some_and(|record| record["urgent"] == Value::Bool(true)))
}
