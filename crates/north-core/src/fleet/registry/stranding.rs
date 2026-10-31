use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::error::NorthResult;
use crate::fleet::model::{EntryKind, Machine, SessionRecord, Status};
use crate::fleet::stranded::{self, Checkout, Lane, Lanes, Stop, Stranded, Worktree};
use crate::fleet::transcript::session_stream_path;

use super::Fleet;

const RECENT: Duration = Duration::from_secs(3 * 24 * 3600);

struct Stopped {
    record: SessionRecord,
    status: Status,
    stop: Stop,
    ended_ms: u64,
    repo: Option<String>,
    trees: Vec<Worktree>,
}

impl Fleet {
    /// The stopped sessions whose last report stopped for capacity or on a pending wait and names a
    /// lane, each with that lane's standing, read in one pass per machine it lives on.
    pub fn stranded(&self, lanes: &dyn Lanes) -> NorthResult<Vec<Stranded>> {
        let stopped = self.stopped(lanes)?;
        let mut read: BTreeMap<Option<String>, (Option<Machine>, Vec<Worktree>)> = BTreeMap::new();
        for session in &stopped {
            let (_, trees) = read.entry(machine_name(&session.record)).or_insert_with(|| (session.record.machine.clone(), Vec::new()));
            for tree in &session.trees {
                if !trees.contains(tree) {
                    trees.push(tree.clone());
                }
            }
        }
        let standing: BTreeMap<(Option<String>, PathBuf), Lane> = read
            .into_iter()
            .flat_map(|(name, (machine, trees))| lanes.lanes(machine.as_ref(), &trees).into_iter().map(move |lane| ((name.clone(), lane.path.clone()), lane)))
            .collect();
        Ok(stopped
            .into_iter()
            .filter_map(|session| {
                let machine = machine_name(&session.record);
                let held: Vec<Lane> = session.trees.iter().filter_map(|tree| standing.get(&(machine.clone(), tree.path.clone())).cloned()).collect();
                (!held.is_empty()).then(|| Stranded {
                    name: session.record.name.clone(),
                    provider: session.record.provider.label().to_owned(),
                    account: session.record.account.clone(),
                    status: session.status.label().to_owned(),
                    repo: session.repo,
                    tasks: session.record.tasks().map(str::to_owned).collect(),
                    machine,
                    stop: session.stop,
                    ended_ms: session.ended_ms,
                    lanes: held,
                })
            })
            .collect())
    }

    /// The exited or parked sessions active in the last three days whose last report stops short
    /// of landing, with the worktrees that report names.
    fn stopped(&self, lanes: &dyn Lanes) -> NorthResult<Vec<Stopped>> {
        let mut checkouts: BTreeMap<(Option<String>, PathBuf), Option<Checkout>> = BTreeMap::new();
        let mut found = Vec::new();
        for record in self.sessions()?.into_iter().filter(recent) {
            let status = self.status(&record);
            if !matches!(status, Status::Exited | Status::Parked) {
                continue;
            }
            let Some((report, ended_ms)) = self.last_report(&record) else { continue };
            let Some(stop) = stranded::stop(&report) else { continue };
            let key = (machine_name(&record), record.cwd.clone());
            let Some(checkout) = checkouts.entry(key).or_insert_with(|| lanes.checkout(record.machine.as_ref(), &record.cwd)) else { continue };
            let trees: Vec<Worktree> = checkout.named(&report).into_iter().cloned().collect();
            if !trees.is_empty() {
                found.push(Stopped { repo: checkout.repo(), record, status, stop, ended_ms, trees });
            }
        }
        Ok(found)
    }

    pub(super) fn last_report(&self, record: &SessionRecord) -> Option<(String, u64)> {
        let follower = self.follow_recent(&record.name).ok()?;
        let transcript = follower.transcript();
        let report = transcript.entries.iter().rfind(|entry| entry.kind == EntryKind::Assistant)?;
        Some((report.text.clone(), transcript.turns.last()?.ended_ms))
    }
}

fn machine_name(record: &SessionRecord) -> Option<String> {
    record.machine.as_ref().map(|machine| machine.name.clone())
}

pub(super) fn recent(record: &SessionRecord) -> bool {
    let modified = std::fs::metadata(session_stream_path(&record.host_dir, &record.name)).and_then(|meta| meta.modified());
    modified.is_ok_and(|at| SystemTime::now().duration_since(at).is_ok_and(|age| age < RECENT))
}
