use std::path::{Path, PathBuf};

use north_base::platform::land_queue;

use super::gc::{ArchivedLane, archive, broken, lane_issues, origin_repo, retire_broken};
use super::lane_rejection;
use super::live_process;
use super::orphan;
use super::reap::irreplaceable_ignored;
use super::repo::{find_container, git_out, is_lander_tree};

const BASES: [&str; 2] = ["refs/remotes/origin/main", "refs/heads/main"];

struct Found {
    registered: bool,
    branch: String,
    tip: String,
}

pub(super) fn reject(lane: &Path, verdict: &str, queues: &Path, archive_root: &Path, now_ms: i64) -> Result<String, String> {
    let verdict = verdict_text(verdict)?;
    let (container, main) = place(lane)?;
    if !land_queue::waiting_on(queues, &land_queue::local_host(), lane).is_empty() {
        return Err("a land ticket waits on it".to_owned());
    }
    let found = find(&main, lane)?;
    if let Some(first) = irreplaceable_ignored(lane).first() {
        return Err(format!("it holds ignored files beyond build output ({first})"));
    }
    if live_process::has_cwd_inside(Path::new("/proc"), lane) {
        lane_rejection::record(&main, &found.branch, &verdict)?;
        return Ok(format!("recorded: {} is rejected ({verdict}); a live process works in it, so north lane gc archives it once nothing holds it", lane.display()));
    }
    let worktree = found.registered.then_some(lane);
    let file = bundle(&container, &main, &found, worktree, &verdict, archive_root, now_ms)?;
    if lane.exists() {
        orphan::remove(&main, lane, queues).map_err(|reason| format!("bundled to {}, but kept the directory: {reason}", file.display()))?;
    }
    Ok(format!("archived: {} to {} (rejected: {verdict})", lane.display(), file.display()))
}

pub(super) fn reject_branch(container: &Path, branch: &str, verdict: &str, archive_root: &Path, now_ms: i64) -> Result<String, String> {
    let verdict = verdict_text(verdict)?;
    let main = container.join("main");
    let found = find_branch(&main, branch)?;
    let retired = archived(container, &main, &found, None, &verdict, |spec| match broken(spec, archive_root) {
        None => Ok(None),
        Some(_) if !names_place(&verdict) => Err("its objects are missing, so it cannot be bundled; name in the verdict where the work is kept (a path)".to_owned()),
        Some(found) => retire_broken(spec, archive_root, &found, now_ms).map(Some),
    })??;
    if let Some(index) = retired {
        return Ok(format!("retired: branch {branch} has missing objects, so nothing was bundled; the work is kept as named in the verdict ({verdict}), recorded in {}", index.display()));
    }
    let file = bundle(container, &main, &found, None, &verdict, archive_root, now_ms)?;
    Ok(format!("archived: branch {branch} to {} (rejected: {verdict})", file.display()))
}

fn verdict_text(verdict: &str) -> Result<String, String> {
    let verdict = verdict.split_whitespace().collect::<Vec<_>>().join(" ");
    if verdict.is_empty() {
        return Err("name the reviewer's verdict after --rejected".to_owned());
    }
    Ok(verdict)
}

fn bundle(container: &Path, main: &Path, found: &Found, worktree: Option<&Path>, verdict: &str, archive_root: &Path, now_ms: i64) -> Result<PathBuf, String> {
    archived(container, main, found, worktree, verdict, |spec| archive(spec, archive_root, now_ms))?
}

fn archived<T>(container: &Path, main: &Path, found: &Found, worktree: Option<&Path>, verdict: &str, run: impl FnOnce(&ArchivedLane) -> T) -> Result<T, String> {
    let base = BASES.iter().find(|base| git_out(main, &["rev-parse", "--verify", "-q", base]).is_some()).ok_or("main has no origin/main or main to bundle against")?;
    let project = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let own = origin_repo(main).unwrap_or_else(|| project.clone());
    let issues = lane_issues(main, &own, base, &found.branch, &found.tip).unwrap_or_default();
    let why = format!("rejected: {verdict}");
    let spec = ArchivedLane { main, base, project: &project, worktree, branch: &found.branch, tip: &found.tip, issues: &issues, why: &why };
    Ok(run(&spec))
}

fn names_place(verdict: &str) -> bool {
    verdict.split_whitespace().any(|word| word.contains('/'))
}

fn place(lane: &Path) -> Result<(PathBuf, PathBuf), String> {
    if !lane.is_absolute() {
        return Err("name it by its absolute path".to_owned());
    }
    let container = lane.parent().filter(|parent| parent.file_name().is_some_and(|name| name == "worktrees")).and_then(Path::parent).ok_or("not a directory directly under a project's worktrees/")?;
    let main = container.join("main");
    if !lane.is_dir() || find_container(lane) != Some(container.to_path_buf()) || is_lander_tree(lane) {
        return Err("not a lane of a project container (no main/.git beside worktrees/)".to_owned());
    }
    Ok((container.to_path_buf(), main))
}

fn find(main: &Path, lane: &Path) -> Result<Found, String> {
    let listing = git_out(main, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let block = listing.split("\n\n").find(|block| block.lines().next().and_then(|line| line.strip_prefix("worktree ")).is_some_and(|path| Path::new(path) == lane));
    if let Some(block) = block {
        if block.lines().any(|line| line == "locked" || line.starts_with("locked ")) {
            return Err("git locks the worktree".to_owned());
        }
        let field = |name: &str| block.lines().find_map(|line| line.strip_prefix(name)).unwrap_or_default().to_owned();
        let branch = field("branch refs/heads/");
        return Ok(Found { registered: true, branch, tip: field("HEAD ") });
    }
    let branch = lane.file_name().unwrap_or_default().to_string_lossy().into_owned();
    match git_out(main, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")]) {
        Some(tip) => Ok(Found { registered: false, branch, tip }),
        None => Err("git no longer registers it and no branch carries its name".to_owned()),
    }
}

fn find_branch(main: &Path, branch: &str) -> Result<Found, String> {
    let listing = git_out(main, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    if listing.lines().any(|line| line.strip_prefix("branch refs/heads/") == Some(branch)) {
        return Err("a worktree has it checked out, so name that worktree's path instead".to_owned());
    }
    match git_out(main, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")]) {
        Some(tip) => Ok(Found { registered: false, branch: branch.to_owned(), tip }),
        None => Err("no local branch carries that name".to_owned()),
    }
}
