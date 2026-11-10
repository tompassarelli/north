use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};

pub type Making = dyn Fn(&Path, &str) -> Result<(), String> + Send + Sync;

/// The lane a box's worker gets beside a primary checkout: `path` under its `worktrees/`, made from `issue` in `checkout`.
pub(super) struct Planned {
    pub checkout: PathBuf,
    pub path: PathBuf,
    pub issue: String,
}

/// The lane planned for a worker named `name` on a box of `tasks` when `cwd` is a primary checkout named `main`, else none.
pub(super) fn planned(cwd: &Path, name: &str, tasks: &[String]) -> Option<Planned> {
    let worktrees = cwd.parent().filter(|_| cwd.file_name().is_some_and(|dir| dir == "main"))?.join("worktrees");
    let issue = tasks.first().map(|task| task.rsplit('/').next().unwrap_or(task))?;
    Some(Planned { checkout: cwd.to_owned(), path: worktrees.join(name), issue: issue.to_owned() })
}

/// The lane a DeepSeek worker starts in: the launcher makes it beside a primary checkout from the box's issue, because the lane-only sandbox cannot run `north lane new` or write the main repository's git directory.
pub(super) fn prepared(cwd: &Path, name: &str, tasks: &[String], make: &Making) -> NorthResult<PathBuf> {
    let Some(lane) = planned(cwd, name, tasks).filter(|lane| lane.path.parent().is_some_and(Path::is_dir)) else { return Ok(cwd.to_owned()) };
    if !lane.path.exists() {
        make(&lane.path, &lane.issue).map_err(|reason| NorthError::Refused(format!("cannot make the lane {} for {name}: {reason}", lane.path.display())))?;
    }
    Ok(lane.path)
}
