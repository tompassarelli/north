use std::collections::HashSet;
use std::path::Path;

use super::candidate::git;
use super::landed_copy;

/// What main already holds of a lane before any of it is picked.
pub enum OnMain {
    Carried(String),
    Nothing,
    AmendedAfterLanding(landed_copy::Landed),
}

fn carrier(worktree: &Path, base: &str, head: &str) -> Option<String> {
    let range = format!("{base}...{head}");
    git(worktree, &["log", "--cherry-mark", "--left-only", "--no-merges", "--format=%m%H", &range]).ok()?.lines().find_map(|line| line.strip_prefix('=').map(str::to_owned))
}

/// Main's hold on the lane from `git cherry base head`, or `None` when the lane has commits to pick.
pub fn judge(worktree: &Path, base: &str, head: &str, cherry: &str) -> Result<Option<OnMain>, String> {
    let unlanded: HashSet<&str> = cherry.lines().filter_map(|line| line.strip_prefix("+ ")).collect();
    if unlanded.is_empty() {
        return Ok(Some(carrier(worktree, base, head).map_or(OnMain::Nothing, OnMain::Carried)));
    }
    let fork = git(worktree, &["merge-base", base, head])?;
    let main = git(worktree, &["log", "--no-merges", landed_copy::FIELDS, &format!("{fork}..{base}")])?;
    let lane = git(worktree, &["log", "--no-merges", landed_copy::FIELDS, &format!("{base}..{head}")])?;
    Ok(landed_copy::amended_after_landing(&main, &lane, &unlanded).map(OnMain::AmendedAfterLanding))
}

pub fn nothing_to_land(head: &str, base: &str) -> String {
    format!("nothing to land: {head} is already on main@{base} and adds no commit to it; commit the lane's change and run safe-push again; NOT landed.")
}
