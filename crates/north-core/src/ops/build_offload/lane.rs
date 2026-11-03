use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::capacity::build_offload::{Declined, names_a_path_here, ships};

/// A worktree as the remote host rebuilds it: the commit, the commits it holds beyond origin/main, and the uncommitted diff.
pub(super) struct Lane {
    pub(super) repo: String,
    pub(super) paths: Vec<String>,
    pub(super) name: String,
    pub(super) head: String,
    pub(super) bundle: Vec<u8>,
    pub(super) patch: Vec<u8>,
    pub(super) subdirectory: String,
}

fn git(directory: &Path, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git").current_dir(directory).args(arguments).stderr(Stdio::null()).output().map_err(|error| format!("git: {error}"))?;
    output.status.success().then_some(output.stdout).ok_or_else(|| format!("git {} failed", arguments.join(" ")))
}

fn text(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    git(directory, arguments).map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
}

/// The full uncommitted change, untracked files included, built in a scratch index so the lane's own index stays as it is.
fn change(top: &Path) -> Result<(Vec<u8>, Vec<String>), String> {
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let index = scratch.path().join("index");
    let indexed = |arguments: &[&str]| -> Result<Vec<u8>, String> {
        let output = Command::new("git").current_dir(top).env("GIT_INDEX_FILE", &index).args(arguments).stderr(Stdio::null()).output().map_err(|error| format!("git: {error}"))?;
        output.status.success().then_some(output.stdout).ok_or_else(|| format!("git {} failed", arguments.join(" ")))
    };
    indexed(&["read-tree", "HEAD"])?;
    indexed(&["add", "-A"])?;
    let patch = indexed(&["diff", "--cached", "--binary", "--full-index", "HEAD"])?;
    let names = indexed(&["diff", "--cached", "--name-only", "-z", "HEAD"])?;
    let changed = String::from_utf8_lossy(&names).split('\0').filter(|name| !name.is_empty()).map(str::to_owned).collect();
    Ok((patch, changed))
}

/// The project's main checkout under HOME, which every host keeps at the same path: the lane's own container holds it as `main` whether the lane is that checkout or a worktree beside it.
fn repository(top: &Path, home: &Path) -> Result<(String, String, PathBuf), String> {
    let beside = top.parent().ok_or("the lane has no parent directory")?;
    let container = if beside.file_name().is_some_and(|name| name == "worktrees") { beside.parent().unwrap_or(beside) } else { beside };
    let main = container.join("main");
    let relative = main.strip_prefix(home).map_err(|_| format!("{} is not under {}", main.display(), home.display()))?.to_string_lossy().into_owned();
    Ok((relative.clone(), relative.replace('/', "-"), container.to_path_buf()))
}

pub(super) fn capture(directory: &Path, command: &[String]) -> Result<Lane, Declined> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| Declined::Failed("HOME is unset".to_owned()))?;
    gather(directory, &home).map_err(Declined::Failed)?.ship(command)
}

pub(super) struct Gathered {
    pub(super) lane: Lane,
    pub(super) changed: Vec<String>,
}

impl Gathered {
    pub(super) fn ship(self, command: &[String]) -> Result<Lane, Declined> {
        let paths: Vec<&str> = self.lane.paths.iter().map(String::as_str).collect();
        names_a_path_here(command, &paths)?;
        let changed: Vec<&str> = self.changed.iter().map(String::as_str).collect();
        ships(&changed, self.lane.patch.len() + self.lane.bundle.len())?;
        Ok(self.lane)
    }
}

pub(super) fn gather(directory: &Path, home: &Path) -> Result<Gathered, String> {
    let top = PathBuf::from(text(directory, &["rev-parse", "--show-toplevel"])?);
    let (repo, name, container) = repository(&top, home)?;
    let head = text(&top, &["rev-parse", "HEAD"])?;
    let base = text(&top, &["merge-base", "HEAD", "origin/main"]).map_err(|_| "the lane has no origin/main to bundle against".to_owned())?;
    let floor = if base == head { format!("{head}~1") } else { base.clone() };
    let bundle = git(&top, &["bundle", "create", "-q", "-", "HEAD", &format!("^{floor}")])?;
    let (patch, mut changed) = change(&top)?;
    let committed = git(&top, &["diff", "--name-only", "-z", &base, "HEAD"])?;
    changed.extend(String::from_utf8_lossy(&committed).split('\0').filter(|name| !name.is_empty()).map(str::to_owned));
    let lane = top.file_name().map(|lane| lane.to_string_lossy().into_owned()).unwrap_or_default();
    let subdirectory = directory.strip_prefix(&top).map(|relative| relative.to_string_lossy().into_owned()).unwrap_or_default();
    let checkouts = container.parent().unwrap_or(&container);
    let projects = checkouts.strip_prefix(home).ok().filter(|relative| !relative.as_os_str().is_empty()).map(|relative| format!("~/{}/", relative.display()));
    let paths = [top.to_string_lossy().into_owned(), format!("{}/", checkouts.display())].into_iter().chain(projects).collect();
    Ok(Gathered { lane: Lane { repo, paths, name: format!("{name}-{lane}"), head, bundle, patch, subdirectory }, changed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_a_lane_names_its_projects_main_checkout_whether_it_is_main_or_a_worktree_beside_it() {
        let home = std::env::var("HOME").unwrap();
        let home = Path::new(&home);
        let (relative, name, _) = repository(&home.join("code/wisp/worktrees/offload-live"), home).unwrap();
        assert_eq!((relative.as_str(), name.as_str()), ("code/wisp/main", "code-wisp-main"));
        assert_eq!(repository(&home.join("code/north/main"), home).unwrap().0, "code/north/main");
    }
}
