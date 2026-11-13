//! The lane each fleet session works in, by the record `north lane new` leaves under .git/north/lane-sessions/SLUG (north#1339).
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde_json::{Map, Value, json};

/// Each fleet session's newest lane that still exists beside the main checkouts `mains`.
pub(super) fn of_sessions(mains: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut newest: HashMap<String, (SystemTime, PathBuf)> = HashMap::new();
    for main in mains {
        let Some(project) = main.parent() else { continue };
        for record in fs::read_dir(main.join(".git/north/lane-sessions")).into_iter().flatten().flatten() {
            let lane = project.join("worktrees").join(record.file_name());
            let (Ok(session), Ok(made)) = (fs::read_to_string(record.path()), record.metadata().and_then(|meta| meta.modified())) else { continue };
            let session = session.trim().to_owned();
            if session.is_empty() || !lane.is_dir() || newest.get(&session).is_some_and(|(seen, _)| *seen >= made) {
                continue;
            }
            newest.insert(session, (made, lane));
        }
    }
    newest.into_iter().map(|(session, (_, lane))| (session, lane)).collect()
}

/// The row's `lane` and `lane_tip`, for a session that made a lane.
pub(super) fn fields(lane: Option<&Path>) -> Map<String, Value> {
    let Some(lane) = lane else { return Map::new() };
    Map::from_iter([("lane".to_owned(), json!(lane.display().to_string())), ("lane_tip".to_owned(), json!(tip(lane)))])
}

fn tip(lane: &Path) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(lane).args(["rev-parse", "HEAD"]).output().ok().filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
