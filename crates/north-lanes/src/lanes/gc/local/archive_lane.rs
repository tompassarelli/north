use super::super::Gc;
use super::super::archive;
use super::{ArchiveLane, Container};

impl Gc {
    pub(super) fn archive_lane(&mut self, sweep: &mut Container, lane: &ArchiveLane) -> bool {
        let why = match lane.rejected {
            Some(verdict) => format!("rejected: {verdict}"),
            None if lane.issues.is_empty() => "no issue, no owner".to_owned(),
            None => format!("issue closed: {}", lane.issues.join(" ")),
        };
        let spec = archive::Lane { main: &sweep.main, base: &sweep.base, project: &sweep.name, worktree: lane.worktree, branch: lane.branch, tip: lane.tip, issues: lane.issues, why: &why };
        if let Some(found) = archive::broken(&spec, &self.options.archive) {
            self.skip_broken(sweep, lane, &spec, &found);
            return false;
        }
        let directory = self.options.archive.join(&sweep.name);
        if self.options.dry_run {
            println!("{}: archive {} ({why}) to {}, then reap", sweep.name, lane.place, self.tilde(&directory));
            self.count_archived(lane);
            return true;
        }
        match archive::archive(&spec, &self.options.archive, self.now_ms) {
            Ok(file) => {
                println!("{}: archived {} ({why}) to {}, then reaped", sweep.name, lane.place, self.tilde(&file));
                sweep.checked_out.remove(lane.branch);
                self.count_archived(lane);
                true
            }
            Err(error) => {
                println!("{}: kept {}: {error}", sweep.name, lane.place);
                false
            }
        }
    }

    fn skip_broken(&mut self, sweep: &Container, lane: &ArchiveLane, spec: &archive::Lane, found: &archive::Broken) {
        if self.options.dry_run {
            println!("{}: broken ref {}: {}; not archived", sweep.name, lane.place, found.detail);
        } else if !found.recorded {
            let noted = archive::note_broken(spec, &self.options.archive, found, self.now_ms);
            println!("{}: broken ref {}: {}; {}", sweep.name, lane.place, found.detail, noted.map_or_else(|error| format!("not recorded ({error})"), |()| "recorded, skipped from now on".to_owned()));
        }
    }

    fn count_archived(&mut self, lane: &ArchiveLane) {
        self.counts.archived += 1;
        self.counts.retired_worktrees += usize::from(lane.worktree.is_some());
        self.counts.deleted_branches += usize::from(!lane.branch.is_empty());
    }
}
