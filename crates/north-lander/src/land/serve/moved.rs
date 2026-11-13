use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::Serve;
use super::batch::Entry;
use crate::land::alone_flight;
use crate::land::bisect;
use crate::land::candidate::{self, git, output};
use crate::land::cause;
use crate::land::jobs;
use crate::land::queue::{self, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::telemetry::Stages;

const SWEPT: &str = "moved-swept";

impl Serve<'_> {
    /// Sends back at once every queued lane that conflicts with main on a path the newest landings deleted or renamed, naming the landing, rather than when the lane reaches the head; true when it sent one back.
    pub(super) fn sweep_moved(&self, alone: Option<&alone_flight::Flight>) -> bool {
        let Ok(queued) = queue::collect(&self.queue) else { return false };
        let claimed = jobs::claimed(self);
        let open: Vec<Entry> = queued.iter().filter(|(path, _)| !claimed.contains(path) && !alone.is_some_and(|flight| flight.checks(path))).map(|(path, ticket)| (path, ticket)).collect();
        if open.is_empty() {
            return false;
        }
        let Ok(main) = candidate::prepare(&self.repo, &self.worktree) else { return false };
        let marker = self.queue.join(SWEPT);
        let last = fs::read_to_string(&marker).ok().map(|text| text.trim().to_owned());
        if last.as_deref() == Some(main.as_str()) || fs::write(&marker, &main).is_err() {
            return false;
        }
        let Some(last) = last else { return false };
        let moved = moved_paths(&self.worktree, &last, &main);
        let mut sent = false;
        for (path, ticket) in open {
            let Some(verdict) = conflict(&self.worktree, &main, ticket, &moved) else { continue };
            let verdict = cause::cite(&self.worktree, &main, &ticket.head, &verdict);
            records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict: &verdict });
            match bisect::decide(path, ticket, verdict, None, &main) {
                Ok(()) => sent = true,
                Err(error) => println!("north land: could not send back {} on the moved paths: {error}", ticket.branch),
            }
        }
        sent
    }
}

fn moved_paths(worktree: &Path, old: &str, new: &str) -> HashSet<String> {
    let listing = git(worktree, &["diff", "--name-status", "-M", "--diff-filter=DR", old, new]).unwrap_or_default();
    listing.lines().filter_map(|line| line.split('\t').nth(1)).map(str::to_owned).collect()
}

fn conflict(worktree: &Path, main: &str, ticket: &Ticket, moved: &HashSet<String>) -> Option<String> {
    if moved.is_empty() || !candidate::readable(worktree, ticket) {
        return None;
    }
    let merged = output(worktree, &["merge-tree", "--write-tree", "--name-only", "--no-messages", main, &ticket.head]).ok()?;
    if merged.status.code() != Some(1) {
        return None;
    }
    let listing = String::from_utf8_lossy(&merged.stdout);
    let files: Vec<&str> = listing.lines().skip(1).take_while(|line| !line.is_empty()).filter(|path| moved.contains(*path)).collect();
    (!files.is_empty()).then(|| format!("conflict {}", files.join(",")))
}

#[cfg(test)]
#[path = "moved_tests.rs"]
pub(super) mod tests;
