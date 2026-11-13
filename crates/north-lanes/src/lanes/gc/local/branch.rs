use super::super::Gc;
use super::super::archive::window_hours;
use super::super::evidence::{LaneFacts, lane_class, lane_issues, protected_branch};
use super::super::judged::Landed;
use super::super::report::Row;
use super::super::super::verdict::{Facts, Verdict, retirement_verdict};
use super::{ArchiveLane, Container, HOUR_MS, cited};
use super::activity::reflog_ms;

impl Gc {
    pub(super) fn sweep_branch(&mut self, sweep: &mut Container, branch: &str, tip: &str) -> Result<(), String> {
        if protected_branch(branch) || sweep.checked_out.contains(branch) {
            return Ok(());
        }
        let verdict = sweep.landed(tip);
        let branch_age = sweep.cutoff_ms - reflog_ms(&sweep.common.join("logs/refs/heads").join(branch));
        if !verdict.landed {
            return self.decide_branch(sweep, branch, tip, &verdict);
        }
        self.delete_branch_after_idle(sweep, branch, tip, branch_age, &verdict.how);
        Ok(())
    }

    fn delete_branch_after_idle(&mut self, sweep: &Container, branch: &str, tip: &str, branch_age: f64, basis: &str) {
        let facts = Facts { age: branch_age.floor() as i64, minimum_age: 0, merged: true, equivalent: false, evil_merge: false, archive_age: None, protections: 0 };
        if retirement_verdict(&facts) == Verdict::Keep {
            self.counts.kept += 1;
        } else if self.options.dry_run {
            println!("{}: delete branch {branch} ({basis})", sweep.name);
            self.counts.deleted_branches += 1;
        } else if self.delete_branch(&sweep.main, branch, tip) {
            self.counts.deleted_branches += 1;
        }
    }

    fn decide_branch(&mut self, sweep: &mut Container, branch: &str, tip: &str, verdict: &Landed) -> Result<(), String> {
        let name = sweep.name.clone();
        let issues = lane_issues(&sweep.main, &sweep.own, &sweep.base, branch, tip).unwrap_or_default();
        let states = self.issue_states(&issues);
        let class = lane_class(&LaneFacts { dirty: false, equivalent: false, issues: &states, idle_hours: 0.0, held: false });
        if let Some(window) = window_hours(&states) {
            let touched_ms = reflog_ms(&sweep.common.join("logs/refs/heads").join(branch)).max(sweep.committed(tip) as f64 * 1000.0);
            let age_h = (self.now_ms as f64 - touched_ms) / HOUR_MS;
            let why = if age_h < window { format!("touched within {} days", window / 24.0) } else { self.local_archive_guard(&sweep.main, &name, &issues, branch, None) };
            if why.is_empty() {
                let found = ArchiveLane { worktree: None, branch, tip, place: &format!("branch {branch}"), issues: &issues, rejected: None };
                if self.archive_lane(sweep, &found) {
                    return Ok(());
                }
            } else if self.options.dry_run {
                println!("{name}: {} branch {branch}: keep ({why})", class.label());
            }
        }
        let row = Row { place: "(branch)", branch, ahead: verdict.ahead, dirty: "", class };
        let hours = self.unlanded_row(&name, &sweep.main, &sweep.base, tip, sweep.committed(tip), &row);
        let owner = self.surface(sweep, "(branch)", branch, hours, &issues)?;
        self.list_open(&issues, &states, &format!("branch {branch}"));
        self.counts.kept += 1;
        if self.options.dry_run {
            println!("{name}: {} branch {branch}: surface to {owner}{}", class.label(), cited(&issues));
        }
        Ok(())
    }
}
