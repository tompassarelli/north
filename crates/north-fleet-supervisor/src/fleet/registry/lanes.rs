use std::fs::File;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};

use super::super::model::SessionRecord;
use super::super::overlap::worktree_in_use;
use super::super::runtime::binary;
use super::super::transcript::{WINDOW_SIZE, read_window, stream_path};
use super::Fleet;

/// Frees the lane a session worked in, on the machine that holds it, once the session has ended.
pub trait RemoteLanes: Send + Sync {
    fn free(&self, record: &SessionRecord) -> NorthResult<()>;
}

/// A remote session's lane lives on its machine, so this frees it over ssh; a local session's lane
/// stays where its worker or `north lane gc` left it.
pub struct OverSshLanes;

impl RemoteLanes for OverSshLanes {
    fn free(&self, record: &SessionRecord) -> NorthResult<()> {
        let Some(machine) = record.machine.as_ref() else { return Ok(()) };
        north_session_host::held::free_lane(&binary("ssh"), &machine.ssh, &record.cwd).map_err(NorthError::State)
    }
}

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
            .filter_map(|record| Some(RunningLane { session: record.name.clone(), task: record.task_ref.clone(), since_ms: record.created_ms, path: worktree_of(record, Path::new(worktrees))? }))
            .collect()
    }
}

/// The worktree under `worktrees` that the tail of the session's output names most, the lane it is working in.
pub fn worktree_of(record: &SessionRecord, worktrees: &Path) -> Option<PathBuf> {
    let mut file = File::open(stream_path(&record.host_dir)).ok()?;
    let end = file.metadata().ok()?.len();
    let tail = String::from_utf8_lossy(&read_window(&mut file, end.saturating_sub(WINDOW_SIZE), WINDOW_SIZE).ok()?).into_owned();
    Some(worktrees.join(worktree_in_use(&tail, &worktrees.to_string_lossy())?))
}
