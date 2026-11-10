//! A retiring or forgotten session's unlanded lanes pass to its named successor or its nearest ancestor still in the fleet,
//! so no lane sits unlanded under a session that can no longer hear the lander (north#941).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

use crate::error::{NorthError, NorthResult};

use super::super::forget::{self, QueuedLane};
use super::super::model::{Delivery, SessionRecord};
use super::super::runtime::{binary, now_ms};
use super::super::sender::Sender;
use super::Fleet;

const HANDED: &str = "lanes-handed.json";

/// What a session owns that has not landed: the tickets that report to it and its own lane worktree.
pub(super) struct OwnedLanes {
    pub(super) tickets: Vec<QueuedLane>,
    worktree: Option<(PathBuf, String)>,
}

impl OwnedLanes {
    pub(super) fn read(queues: &Path, record: &SessionRecord) -> Self {
        let tickets = forget::queued_lanes(queues, &record.name, Some(&record.cwd));
        let worktree = unlanded_branch(&record.cwd).filter(|branch| tickets.iter().all(|lane| &lane.branch != branch)).map(|branch| (record.cwd.clone(), branch));
        Self { tickets, worktree }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.tickets.is_empty() && self.worktree.is_none()
    }

    fn describe(&self) -> String {
        let tickets = self.tickets.iter().map(|lane| format!("queued lane {}", lane.branch));
        let worktree = self.worktree.iter().map(|(path, branch)| format!("unlanded branch {branch} in {}", path.display()));
        tickets.chain(worktree).collect::<Vec<_>>().join(", ")
    }
}

fn unlanded_branch(cwd: &Path) -> Option<String> {
    if !cwd.join(".git").is_file() {
        return None;
    }
    let git = |args: &[&str]| Command::new(binary("git")).arg("-C").arg(cwd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|ran| ran.status.success()).map(|ran| String::from_utf8_lossy(&ran.stdout).into_owned());
    let branch = git(&["symbolic-ref", "--quiet", "--short", "HEAD"])?.trim().to_owned();
    let ahead = git(&["cherry", "origin/main", "HEAD"])?.lines().any(|line| line.starts_with('+'));
    let dirty = !git(&["status", "--porcelain"])?.trim().is_empty();
    (ahead || dirty).then_some(branch)
}

impl Fleet {
    /// Who takes over `record`'s lanes: the named `successor`, which must be in the fleet, else its nearest ancestor still there.
    pub(super) fn lane_heir(&self, record: &SessionRecord, successor: Option<&str>) -> NorthResult<Option<String>> {
        let heir = match successor {
            Some(named) if self.session_dir(named)?.exists() || self.known_elsewhere(named)? => Some(named.to_owned()),
            Some(named) => return Err(NorthError::Usage(format!("{named} is no session in this fleet, so it cannot take over {}'s lanes", record.name))),
            None => self.nearest_heir(record.clone())?,
        };
        Ok(heir.filter(|heir| heir != &record.name))
    }

    /// Hands every unlanded lane `record` owns to its heir: each waiting ticket now reports to the heir, who is told the branches and worktree; whether a lane was handed over.
    /// With no heir, `strict` refuses; otherwise the lanes stay where they lie.
    pub(super) fn hand_over_lanes(&self, record: &SessionRecord, successor: Option<&str>, strict: bool) -> NorthResult<bool> {
        let dir = self.session_dir(&record.name)?;
        if dir.join(HANDED).exists() {
            return Ok(true);
        }
        let lanes = OwnedLanes::read(&self.state.join("land-queue"), record);
        if lanes.is_empty() {
            return Ok(false);
        }
        let Some(heir) = self.lane_heir(record, successor)? else {
            return if strict {
                Err(NorthError::Refused(format!("{} owns {} and has no parent or successor to take it; land it, or name a successor with --to NAME", record.name, lanes.describe())))
            } else {
                Ok(false)
            };
        };
        let text = format!("[north: {} is leaving the fleet and its lanes are now yours: {}. Land each through safe-push or abandon it; a queued lane's verdict now comes to you.]", record.name, lanes.describe());
        self.send_signed(&Sender::fleet(), &heir, &text, Delivery::Queued)?;
        for lane in &lanes.tickets {
            forget::reassign(&lane.ticket, &heir)?;
        }
        let branches: Vec<&str> = lanes.tickets.iter().map(|lane| lane.branch.as_str()).chain(lanes.worktree.iter().map(|(_, branch)| branch.as_str())).collect();
        fs::write(dir.join(HANDED), serde_json::to_vec_pretty(&json!({"to": heir, "at_ms": now_ms(), "branches": branches}))?)?;
        Ok(true)
    }
}
