//! The landing tickets a session enqueued that have not landed, which hold the session listed.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::NorthResult;

const DONE: &str = "done";
const WITHDRAWN: &str = "withdrawn";
const QUARANTINE: &str = "quarantine";
const LANDED: &str = "landed ";

pub trait Tickets: Send + Sync {
    /// The branches whose newest ticket from this session has not landed.
    fn unlanded(&self, session: &str) -> NorthResult<Vec<String>>;
}

pub struct NoTickets;

impl Tickets for NoTickets {
    fn unlanded(&self, _session: &str) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// The lander's queues on this machine: one directory per repository, its pending tickets at the top and its settled ones in areas below.
pub struct LandQueue {
    root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Standing {
    Open,
    Settled,
}

#[derive(Clone, Copy)]
enum Area {
    Pending,
    Done,
    Withdrawn,
    Quarantine,
}

impl Area {
    const ALL: [Self; 4] = [Self::Pending, Self::Done, Self::Withdrawn, Self::Quarantine];

    fn directory(self, queue: &Path) -> PathBuf {
        match self {
            Self::Pending => queue.to_path_buf(),
            Self::Done => queue.join(DONE),
            Self::Withdrawn => queue.join(WITHDRAWN),
            Self::Quarantine => queue.join(QUARANTINE),
        }
    }

    fn standing(self, verdict: Option<&str>) -> Standing {
        match self {
            Self::Done if verdict.is_some_and(|verdict| verdict.starts_with(LANDED)) => Standing::Settled,
            Self::Withdrawn => Standing::Settled,
            Self::Pending | Self::Done | Self::Quarantine => Standing::Open,
        }
    }
}

impl LandQueue {
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn here() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|path| path.is_absolute());
        let state = state.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        Self::at(state.join("north/land-queue"))
    }
}

impl Tickets for LandQueue {
    fn unlanded(&self, session: &str) -> NorthResult<Vec<String>> {
        let mut newest: BTreeMap<(PathBuf, String), (u128, Standing)> = BTreeMap::new();
        let queues = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        for queue in queues {
            let queue = queue?.path();
            if !queue.is_dir() {
                continue;
            }
            for area in Area::ALL {
                for (queued, ticket) in tickets_in(&area.directory(&queue))? {
                    if ticket.get("session").and_then(Value::as_str) != Some(session) {
                        continue;
                    }
                    let branch = ticket.get("branch").and_then(Value::as_str).unwrap_or_default().to_owned();
                    let standing = area.standing(ticket.get("verdict").and_then(Value::as_str));
                    let slot = newest.entry((queue.clone(), branch)).or_insert((queued, standing));
                    if queued >= slot.0 {
                        *slot = (queued, standing);
                    }
                }
            }
        }
        Ok(newest.into_iter().filter(|(_, (_, standing))| *standing == Standing::Open).map(|((_, branch), _)| branch).collect())
    }
}

fn tickets_in(directory: &Path) -> NorthResult<Vec<(u128, Value)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut tickets = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !path.is_file() || path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let queued = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('-').next()).and_then(|micros| micros.parse().ok()).unwrap_or(u128::MAX);
        tickets.push((queued, serde_json::from_str(&fs::read_to_string(&path)?)?));
    }
    Ok(tickets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(queue: &Path, area: Option<&str>, queued: u128, branch: &str, session: &str, verdict: Option<&str>) {
        let directory = area.map_or_else(|| queue.to_path_buf(), |area| queue.join(area));
        fs::create_dir_all(&directory).unwrap();
        let body = serde_json::json!({"branch": branch, "session": session, "verdict": verdict});
        fs::write(directory.join(format!("{queued}-{branch}.json")), body.to_string()).unwrap();
    }

    #[test]
    fn a_branch_is_unlanded_while_its_newest_ticket_from_the_session_is_pending_red_or_quarantined() {
        let root = tempfile::tempdir().unwrap();
        let queue = root.path().join("560e3f31dbaa2415");
        ticket(&queue, Some(DONE), 1, "claude-land-a", "worker", Some("red: tests failed"));
        ticket(&queue, Some(DONE), 2, "claude-land-a", "worker", Some("landed abc123"));
        ticket(&queue, Some(DONE), 3, "claude-land-b", "worker", Some("landed def456"));
        ticket(&queue, None, 4, "claude-land-b", "worker", None);
        ticket(&queue, Some(DONE), 5, "claude-land-c", "worker", Some("red: clippy"));
        ticket(&queue, Some(QUARANTINE), 6, "claude-land-d", "worker", Some("unreadable head"));
        ticket(&queue, Some(WITHDRAWN), 7, "claude-land-e", "worker", Some("withdrawn by the lead"));
        ticket(&queue, None, 8, "claude-land-f", "other", None);
        let unlanded = LandQueue::at(root.path().to_path_buf()).unlanded("worker").unwrap();
        assert_eq!(unlanded, ["claude-land-b", "claude-land-c", "claude-land-d"]);
        assert!(LandQueue::at(root.path().join("absent")).unlanded("worker").unwrap().is_empty());
    }
}
