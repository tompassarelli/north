//! The checkout a request to the outside broker is asked from (south#118). The process the broker starts runs as the operator outside the worker's domain, and the git it runs reads the repository its working directory names, so a request is served only from a lane or a main checkout of a project this host keeps, or from a slot the build offload made for one, and only while that checkout reads its project's own repository. A directory the broker cannot place in one is refused for every verb. Where git reads from is the repository the project's main checkout keeps, which no worker writes: a lane's own `.git` is read to refuse a request, never to name a repository.

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

/// Where this host's build offload keeps its slots: the `NORTH_OFFLOAD_ROOT` of `environment`, the broker's own, else `.cache/land-offload` under `home`, as the offload's script reads it.
pub(super) fn slots(environment: &[(String, String)], home: &Path) -> PathBuf {
    use north_base::foundation::offload::{ROOT_DEFAULT, ROOT_ENV};
    environment.iter().find(|(name, value)| name == ROOT_ENV && !value.is_empty()).map_or_else(|| home.join(ROOT_DEFAULT), |(_, root)| PathBuf::from(root))
}

/// The checkout a request asked from `directory` is served from, or why it is refused: the lane or main checkout `registered` places it in, else the build offload's slot under `slots` it lies in. A job the offload runs asks from its slot, which lies under no project.
pub(super) fn served(code: &[PathBuf], slots: &Path, directory: &Path) -> Result<Checkout, String> {
    registered(code, directory).or_else(|refusal| slot(code, slots, directory).ok_or(refusal))
}

/// The slot `SLOTS/NAME` that `directory` lies in, as the checkout of the one project under `code` whose main checkout keeps the worktree `NAME` for it: the offload's script makes a slot with `git worktree add` in that repository (land_offload/ship/script.rs), and the slot's `.git` is read only to tell whether it leads there. `None` for a directory in no such slot, whatever its name.
fn slot(code: &[PathBuf], slots: &Path, directory: &Path) -> Option<Checkout> {
    let (real, slots) = (directory.canonicalize().ok()?, slots.canonicalize().ok()?);
    let top = slots.join(real.strip_prefix(&slots).ok()?.components().next()?);
    let read = git_dirs(&top)?;
    let mains = code.iter().filter_map(|code| std::fs::read_dir(code.canonicalize().ok()?).ok()).flatten().flatten().map(|project| project.path().join("main"));
    let kept = mains.filter_map(|main| main_git_dirs(&main)).map(|(_, common)| (common.join("worktrees").join(top.file_name().unwrap_or_default()), common)).find(|kept| *kept == read)?;
    Some(Checkout { top, git_dir: kept.0, common: kept.1 })
}
