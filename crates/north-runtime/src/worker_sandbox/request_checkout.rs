//! The checkout a request to the outside broker is asked from (south#118). The process the broker starts runs as the operator outside the worker's domain, and the git it runs reads the repository its working directory names, so a request is served only from a lane or a main checkout of a project this host keeps, and only while that checkout reads its project's own repository. A directory the broker cannot place in one is refused for every verb. Where git reads from is the repository the project's main checkout keeps, which no worker writes: a lane's own `.git` is read to refuse a request, never to name a repository.

use std::path::{Path, PathBuf};

use super::removable::{git_dirs, main_git_dirs};

/// The variables that tell git where a repository is; a process started with all three reads no `.git` and no `commondir` to find one.
pub(super) const PLACES: [&str; 3] = ["GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE"];

/// The lane or main checkout a request is served from: its top, and the git directory and common directory the project's main checkout keeps for it.
pub(super) struct Checkout {
    pub top: PathBuf,
    pub git_dir: PathBuf,
    pub common: PathBuf,
}

impl Checkout {
    /// The variables that make every git a process runs read this checkout's repository, whatever its `.git` says by then. They reach each git the process starts, in any directory, so they suit only a process whose git runs in this checkout alone.
    pub(super) fn pinned(&self) -> [(String, String); 3] {
        let places = [&self.git_dir, &self.common, &self.top];
        std::array::from_fn(|at| (PLACES[at].to_owned(), places[at].to_string_lossy().into_owned()))
    }
}

/// The project under `code` that `directory` lies in and the top of its lane (`CODE/PROJECT/worktrees/NAME`) or main checkout (`CODE/PROJECT/main`) there, `None` anywhere else; both are canonical.
fn top(code: &Path, directory: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut below = directory.strip_prefix(code).ok()?.components();
    let project = code.join(below.next()?);
    let top = match below.next()?.as_os_str().to_str()? {
        "main" => project.join("main"),
        "worktrees" => project.join("worktrees").join(below.next()?),
        _ => return None,
    };
    Some((project, top))
}

/// The checkout a request asked from `directory` is served from, on a host whose projects lie under `code`, or why it is refused: the directory is in no lane and no main checkout of them, its project has no main checkout with a repository of its own, or the lane's `.git` does not lead to the worktree of its own name in that repository.
pub(super) fn registered(code: &[PathBuf], directory: &Path) -> Result<Checkout, String> {
    let unplaced = || format!("{} is in no lane (PROJECT/worktrees/NAME) and no main checkout (PROJECT/main) of this host's projects, and a request is served only from one; run it from your lane", directory.display());
    let real = directory.canonicalize().map_err(|_| unplaced())?;
    let (project, top) = code.iter().filter_map(|code| code.canonicalize().ok()).find_map(|code| top(&code, &real)).ok_or_else(unplaced)?;
    let main = project.join("main");
    let (main_git_dir, common) = main_git_dirs(&main).ok_or_else(|| format!("{} is no main checkout with a repository of its own, so nothing says which repository {} reads, and a request is served only from a lane as `north lane new` made it", main.display(), top.display()))?;
    if top == main {
        return Ok(Checkout { top, git_dir: main_git_dir, common });
    }
    let git_dir = common.join("worktrees").join(top.file_name().unwrap_or_default());
    match git_dirs(&top) {
        Some((read, shared)) if read == git_dir && shared == common => Ok(Checkout { top, git_dir, common }),
        Some((_, shared)) if shared != common => Err(format!("{} is not a checkout of {}: its .git names another repository, and a request is served only from a lane as `north lane new` made it", top.display(), main.display())),
        _ => Err(format!("{} is not the worktree {} keeps for it: its .git does not lead to {}, and a request is served only from a lane as `north lane new` made it", top.display(), main.display(), git_dir.display())),
    }
}
