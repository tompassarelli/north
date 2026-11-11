use std::fs::File;
use std::path::{Path, PathBuf};

use north_session_host::held::{KeptLane, kept_lanes};

use crate::error::{NorthError, NorthResult};

use super::super::model::SessionRecord;
use super::super::overlap::worktree_in_use;
use super::super::runtime::binary;
use super::super::transcript::{WINDOW_SIZE, read_window, stream_path};
use super::Fleet;

pub(super) const KEPT_LANES: &str = ".kept-lanes.tsv";

/// The worktree a remote session worked in, on the machine that holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteLane {
    pub machine: String,
    pub ssh: String,
    pub path: PathBuf,
}

/// Frees the lane a session worked in, on the machine that holds it, once the session has ended.
pub trait RemoteLanes: Send + Sync {
    fn free(&self, lane: &RemoteLane) -> NorthResult<()>;
}

/// A remote session's lane lives on its machine, so this frees it over ssh; a local session's lane
/// stays where its worker or `north lane gc` left it.
pub struct OverSshLanes {
    pub kept: PathBuf,
}

impl RemoteLanes for OverSshLanes {
    fn free(&self, lane: &RemoteLane) -> NorthResult<()> {
        north_session_host::held::free_lane(&binary("ssh"), &lane.ssh, &lane.machine, &lane.path, &self.kept).map_err(NorthError::State)
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
    /// The lane `record` worked in on its machine, read while its output is still in the registry: its launch
    /// directory when that is a worktree, else the worktree beside its checkout that its output names most, since a
    /// worker launched in the main checkout makes its lane there; none for a local session, or for a lane another
    /// session on that machine works in.
    pub(super) fn remote_lane(&self, record: &SessionRecord) -> Option<RemoteLane> {
        let machine = record.machine.as_ref()?;
        let parent = record.cwd.parent()?;
        let path = if parent.file_name().is_some_and(|name| name == "worktrees") { record.cwd.clone() } else { worktree_of(record, &parent.join("worktrees"))? };
        let worktrees = path.parent()?;
        let sessions = self.sessions().ok()?;
        let shared = sessions.iter().any(|other| {
            other.name != record.name && other.machine.as_ref().is_some_and(|held| held.name == machine.name) && (other.cwd == path || worktree_of(other, worktrees).as_ref() == Some(&path))
        });
        (!shared).then(|| RemoteLane { machine: machine.name.clone(), ssh: machine.ssh.clone(), path })
    }

    /// The lanes freeing kept since `since_secs`, with the machine, the lane and the reason `north lane remove` gave.
    pub fn kept_lanes_since(&self, since_secs: u64) -> Vec<KeptLane> {
        kept_lanes(&self.root.join(KEPT_LANES), since_secs)
    }

    pub(super) fn free_remote_lane(&self, session: &str, lane: Option<&RemoteLane>) {
        if let Some(lane) = lane
            && let Err(error) = self.remote_lanes.free(lane)
        {
            eprintln!("north fleet: {session}'s lane {} on {} is left for north lane gc: {}", lane.path.display(), lane.machine, error.user_message());
        }
    }

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
