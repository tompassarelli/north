//! What opencode records in its own session store that its `run --format json` stream never shows: the compactions
//! of a session, each a session message of type `compaction` (north#35).
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

const STORE: &str = "data/opencode/opencode.db";

/// The compactions `session` recorded since `since_ms` in the store under the host's `scratch`; none when the store or
/// the session cannot be read.
pub(super) fn compactions(scratch: &Path, session: &str, since_ms: u64) -> Option<u64> {
    let store = Connection::open_with_flags(scratch.join(STORE), OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let since = i64::try_from(since_ms).unwrap_or(i64::MAX);
    let counted: i64 = store
        .query_row(
            "SELECT (SELECT count(*) FROM session_message WHERE session_id = ?1 AND type = 'compaction' AND time_created >= ?2) FROM session_v2 WHERE id = ?1",
            params![session, since],
            |row| row.get(0),
        )
        .ok()?;
    u64::try_from(counted).ok()
}
