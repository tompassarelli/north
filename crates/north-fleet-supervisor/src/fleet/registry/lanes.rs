use std::fs::File;
use std::path::PathBuf;

use super::Fleet;
use crate::fleet::overlap::worktree_in_use;
use crate::fleet::transcript::{WINDOW_SIZE, read_window, stream_path};

/// A running session and the worktree it is working in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunningLane {
    pub session: String,
    pub task: Option<String>,
    pub since_ms: u64,
    pub path: PathBuf,
}

impl Fleet {
    /// The worktrees under `worktrees` that this host's running sessions, except `except`, work in, read from the tail of each session's output.
    pub fn running_lanes(&self, worktrees: &str, except: &str) -> Vec<RunningLane> {
        let Ok(sessions) = self.sessions() else { return Vec::new() };
        sessions
            .iter()
            .filter(|record| record.name != except && record.machine.is_none() && !record.verification && self.running(record))
            .filter_map(|record| {
                let mut file = File::open(stream_path(&record.host_dir)).ok()?;
                let end = file.metadata().ok()?.len();
                let tail = String::from_utf8_lossy(&read_window(&mut file, end.saturating_sub(WINDOW_SIZE), WINDOW_SIZE).ok()?).into_owned();
                let slug = worktree_in_use(&tail, worktrees)?;
                Some(RunningLane { session: record.name.clone(), task: record.task_ref.clone(), since_ms: record.created_ms, path: PathBuf::from(worktrees).join(slug) })
            })
            .collect()
    }
}
