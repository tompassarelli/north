use std::path::Path;

use north_base::platform::{host, records_mirror};

pub struct Reverted<'a> {
    pub branch: &'a str,
    pub commit: &'a str,
    pub revert: &'a str,
    pub cause: Cause<'a>,
}

/// Why a landed commit was reverted: its post-land checks failed at `red`.
pub enum Cause<'a> {
    RedMain { red: &'a str, failing: &'a str, log: &'a Path },
}

impl Reverted<'_> {
    fn short(&self) -> &str {
        &self.commit[..self.commit.len().min(9)]
    }

    fn evidence(&self) -> String {
        match &self.cause {
            Cause::RedMain { red, failing, log } => format!("the post-land checks failed at {} on {failing}; they failed twice at the tip and passed twice on main without {} ({}) in back-to-back reruns, so the sheriff queued its revert {} ahead of the queue; log: {}", &red[..red.len().min(9)], self.branch, self.short(), self.revert, readable(log)),
        }
    }

    /// The line the lander's journal carries for a reverted lane, the one place it goes since the fleet's messages and relaunches were removed (north#1650).
    pub fn notice(&self) -> String {
        format!("north land: {}; fix forward: start a lane from origin/main, bring the work back with `git cherry-pick {}`, fix what fails, and hand it in with Follows: {}", self.evidence(), self.commit, self.short())
    }
}

fn readable(log: &Path) -> String {
    let state = records_mirror::state_home();
    match log.strip_prefix(&state).map(|rest| state.join(records_mirror::ROOT).join(rest)) {
        Ok(copy) => format!("{} on {}, or its records-mirror copy {} on any other host", log.display(), host::name(), copy.display()),
        Err(_) => log.display().to_string(),
    }
}

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
