use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};

pub type Making = dyn Fn(&Path, &str) -> Result<(), String> + Send + Sync;

/// The lane a DeepSeek worker starts in: the launcher makes it beside a primary checkout from the box's issue, because the lane-only sandbox cannot run `north lane new` or write the main repository's git directory.
pub(super) fn prepared(cwd: &Path, name: &str, tasks: &[String], make: &Making) -> NorthResult<PathBuf> {
    let Some(worktrees) = cwd.parent().map(|container| container.join("worktrees")).filter(|worktrees| cwd.file_name().is_some_and(|dir| dir == "main") && worktrees.is_dir()) else {
        return Ok(cwd.to_owned());
    };
    let Some(issue) = tasks.first().map(|task| task.rsplit('/').next().unwrap_or(task)) else { return Ok(cwd.to_owned()) };
    let lane = worktrees.join(name);
    if !lane.exists() {
        make(&lane, issue).map_err(|reason| NorthError::Refused(format!("cannot make the lane {} for {name}: {reason}", lane.display())))?;
    }
    Ok(lane)
}
