use std::path::Path;

use north_base::platform::land_queue::{self, LandedLane};
use north_base::platform::records_mirror::Records;

use super::repo::{git_ok, git_out};

/// The lanes the lander recorded as landed from this host, read from this host's queue, the verdicts sent back to its
/// outbox and the records mirror: the positive proof a squash-landed lane is finished when its tip is not on main.
pub(super) struct LandedTickets {
    lanes: Vec<LandedLane>,
}

impl LandedTickets {
    pub(super) fn read(state_home: &Path, main: &Path) -> Self {
        let Some(origin) = git_out(main, &["remote", "get-url", "origin"]) else { return Self { lanes: Vec::new() } };
        let host = land_queue::local_host();
        let mut queues = vec![land_queue::directory(state_home, &origin), land_queue::directory(Records::of(state_home, &host).home(), &origin)];
        queues.dedup();
        Self { lanes: land_queue::landed_lanes(&queues, &host) }
    }

    pub(super) fn roots(&self) -> impl Iterator<Item = &Path> {
        self.lanes.iter().map(|ticket| ticket.root.as_path())
    }

    /// The commit the lander made on `base` for the worktree at `lane` whose head is `head`, when a landed ticket
    /// names this lane with that same head and that commit is on `base`; the caller has checked the worktree is clean.
    pub(super) fn landed(&self, main: &Path, base: &str, lane: &Path, head: &str) -> Option<&str> {
        let here = std::fs::canonicalize(lane).ok();
        let names_lane = |root: &Path| root == lane || here.is_some() && std::fs::canonicalize(root).ok() == here;
        let ticket = self.lanes.iter().filter(|ticket| ticket.head == head && names_lane(&ticket.root)).find(|ticket| git_ok(main, &["merge-base", "--is-ancestor", &ticket.commit, base]))?;
        Some(&ticket.commit)
    }
}
