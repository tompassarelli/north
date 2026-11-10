use std::path::{Path, PathBuf};

use north_base::platform::land_queue;

use super::gc::{ArchivedLane, archive, lane_issues, origin_repo};
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
    let verdict = verdict.split_whitespace().collect::<Vec<_>>().join(" ");
    if verdict.is_empty() {
        return Err("name the reviewer's verdict after --rejected".to_owned());
    }
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
    let base = BASES.iter().find(|base| git_out(&main, &["rev-parse", "--verify", "-q", base]).is_some()).ok_or("main has no origin/main or main to bundle against")?;
    let project = container.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let own = origin_repo(&main).unwrap_or_else(|| project.clone());
    let issues = lane_issues(&main, &own, base, &found.branch, &found.tip).unwrap_or_default();
    let why = format!("rejected: {verdict}");
    let spec = ArchivedLane { main: &main, base, project: &project, worktree: found.registered.then_some(lane), branch: &found.branch, tip: &found.tip, issues: &issues, why: &why };
    let bundle = archive(&spec, archive_root, now_ms)?;
    if lane.exists() {
        orphan::remove(&main, lane, queues).map_err(|reason| format!("bundled to {}, but kept the directory: {reason}", bundle.display()))?;
    }
    Ok(format!("archived: {} to {} ({why})", lane.display(), bundle.display()))
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
