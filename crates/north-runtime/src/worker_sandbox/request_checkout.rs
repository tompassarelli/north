//! The checkout a request to the outside broker is asked from (south#118). The process the broker starts runs as the operator outside the worker's domain, and the git it runs reads the repository its working directory names, so a request is served only from a lane or a main checkout of a project this host keeps, or from a slot the build offload recorded as one's, and only while that checkout reads its project's own repository. A directory the broker cannot place in one is refused for every verb. Where git reads from is the repository the project's main checkout keeps, which no worker writes: a lane's own `.git` is read to refuse a request, never to name a repository.

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

/// The project container under one of `code` that `directory` lies in, and the lane (`CODE/PROJECT/worktrees/NAME`) it lies in when it lies in one, both canonical: the directories a broker serves are `~/code` and the one it was started to serve beside it, such as an evaluation's work root.
pub(super) fn project(code: &[PathBuf], directory: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let real = directory.canonicalize().unwrap_or_else(|_| directory.to_path_buf());
    code.iter().filter_map(|code| code.canonicalize().ok()).find_map(|code| {
        let mut below = real.strip_prefix(&code).ok()?.components();
        let project = code.join(below.next()?);
        let lane = match (below.next(), below.next()) {
            (Some(worktrees), Some(name)) if worktrees.as_os_str() == "worktrees" => Some(project.join("worktrees").join(name)),
            _ => None,
        };
        Some((project, lane))
    })
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

/// This host's build offload slots: the directory that holds them (the `NORTH_OFFLOAD_ROOT` of the broker's own environment, else the default under `home`) and the one where the offload's script records the checkout each slot belongs to (land_offload/ship/script.rs), which no worker command writes.
pub(super) struct Slots {
    root: PathBuf,
    records: PathBuf,
    home: PathBuf,
}

impl Slots {
    pub(super) fn of(environment: &[(String, String)], home: &Path) -> Self {
        use north_base::foundation::offload::{ROOT_DEFAULT, ROOT_ENV, SLOT_RECORDS};
        let root = environment.iter().find(|(name, value)| name == ROOT_ENV && !value.is_empty()).map_or_else(|| home.join(ROOT_DEFAULT), |(_, root)| PathBuf::from(root));
        Self { root, records: home.join(SLOT_RECORDS), home: home.to_owned() }
    }

    /// The slot `ROOT/NAME` that `directory` lies in, as a checkout of the project the offload's script recorded for that name: the record names the main checkout of a project under `code`, whose repository keeps the worktree `NAME`. The slots lie where a worker writes, so nothing in a slot is read, its `.git` least of all, and a name the project has a lane of is no slot's: that worktree is the lane's. `None` for any other directory.
    fn checkout(&self, code: &[PathBuf], directory: &Path) -> Option<Checkout> {
        let (real, root) = (directory.canonicalize().ok()?, self.root.canonicalize().ok()?);
        let top = root.join(real.strip_prefix(&root).ok()?.components().next()?);
        let name = top.file_name()?;
        let recorded = std::fs::read_to_string(self.records.join(name)).ok()?;
        let main = self.home.join(recorded.trim()).canonicalize().ok()?;
        let (project, _) = code.iter().filter_map(|code| code.canonicalize().ok()).find_map(|code| self::top(&code, &main)).filter(|(project, placed)| *placed == main && main == project.join("main"))?;
        let (_, common) = main_git_dirs(&main).filter(|_| project.join("worktrees").join(name).symlink_metadata().is_err())?;
        let git_dir = Some(common.join("worktrees").join(name)).filter(|kept| kept.is_dir())?;
        Some(Checkout { top, git_dir, common })
    }
}

/// The checkout a request asked from `directory` is served from, or why it is refused: the lane or main checkout `registered` places it in, else the build offload's slot it lies in. A job the offload runs asks from its slot, which lies under no project.
pub(super) fn served(code: &[PathBuf], slots: &Slots, directory: &Path) -> Result<Checkout, String> {
    registered(code, directory).or_else(|refusal| slots.checkout(code, directory).ok_or(refusal))
}
