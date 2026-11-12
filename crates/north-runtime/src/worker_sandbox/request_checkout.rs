//! The checkout a request to the outside broker is asked from (south#118). The process the broker starts runs as the operator outside the worker's domain, and the git it runs reads the repository its working directory names, so a request asked from a lane or a main checkout is served only while that checkout reads its project's own repository.

use std::path::{Path, PathBuf};

use super::removable::{foreign, lane, project};

/// The project under `home` that `directory` lies in and the top of its lane (`~/code/PROJECT/worktrees/NAME`) or main checkout (`~/code/PROJECT/main`) there, `None` anywhere else; `home` and `directory` are canonical.
pub(super) fn top(home: &Path, directory: &Path) -> Option<(PathBuf, PathBuf)> {
    let project = project(home, directory)?;
    let main = Some(project.join("main")).filter(|main| directory.starts_with(main));
    lane(home, directory).or(main).map(|top| (project, top))
}

/// Why a request asked from `directory`, on a host whose home is `home`, is refused, if it is: the lane or main checkout it lies in reads a repository other than its project's own.
pub(super) fn refusal(home: &Path, directory: &Path) -> Option<String> {
    let (home, real) = home.canonicalize().ok().zip(directory.canonicalize().ok())?;
    let (project, top) = top(&home, &real)?;
    foreign(&project, &top).then(|| format!("{} is not a checkout of {}: its .git names another repository, and a request is served only from a lane as `north lane new` made it", top.display(), project.join("main").display()))
}
