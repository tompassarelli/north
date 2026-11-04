use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::queue::Ticket;
use crate::landing::stale_revert;

pub const BRANCH: &str = "land-candidate";

pub fn output(directory: &Path, arguments: &[&str]) -> Result<Output, String> {
    Command::new("git").arg("-C").arg(directory).args(arguments).env("GIT_EDITOR", "true").output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))
}

pub fn git(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = output(directory, arguments)?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn succeeds(directory: &Path, arguments: &[&str]) -> bool {
    output(directory, arguments).is_ok_and(|output| output.status.success())
}

pub fn lander_worktree(repo: &Path) -> Result<PathBuf, String> {
    let common = PathBuf::from(git(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    let container = common.parent().and_then(Path::parent).ok_or_else(|| format!("{} has no project container", common.display()))?;
    Ok(container.join("worktrees/lander"))
}

pub fn prepare(repo: &Path, worktree: &Path) -> Result<String, String> {
    if !worktree.join(".git").exists() {
        let path = worktree.to_str().ok_or_else(|| format!("{} is not UTF-8", worktree.display()))?;
        git(repo, &["worktree", "add", "-q", "--detach", path, "HEAD"])?;
    }
    if worktree.join(".git").is_file() && succeeds(worktree, &["rev-parse", "-q", "--verify", "CHERRY_PICK_HEAD"]) {
        git(worktree, &["cherry-pick", "--abort"])?;
    }
    git(worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    git(worktree, &["rev-parse", "FETCH_HEAD^{commit}"])
}

pub enum Pick {
    Included,
    Decided(String),
    Undecided(String),
}

pub struct Built {
    pub head: String,
    pub picks: Vec<Pick>,
}

impl Built {
    pub fn included(&self) -> usize {
        self.picks.iter().filter(|pick| matches!(pick, Pick::Included)).count()
    }
}

fn has_commit(worktree: &Path, sha: &str) -> bool {
    succeeds(worktree, &["cat-file", "-e", &format!("{sha}^{{commit}}")])
}

fn dropped_refusal(base: &str, commits: &str) -> String {
    format!("lane commits missing from the candidate after the pick onto main@{base}: {commits}; NOT landed.")
}

fn refuse(logs: &Path, name: &str, kind: &str, text: &str) -> Result<Pick, String> {
    fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = logs.join(format!("{name}.{kind}.log"));
    fs::write(&log, format!("{text}\n")).map_err(|error| format!("{}: {error}", log.display()))?;
    Ok(Pick::Decided(format!("failed {}", log.display())))
}

fn pick(worktree: &Path, base: &str, ticket: &Ticket, logs: &Path, name: &str) -> Result<Pick, String> {
    if !has_commit(worktree, &ticket.head) {
        let root = ticket.root.to_str().unwrap_or_default();
        let _ = output(worktree, &["fetch", "-q", "--no-tags", root, &format!("refs/heads/{}", ticket.branch)]);
        if !has_commit(worktree, &ticket.head) {
            return Ok(Pick::Decided("conflict unreadable".to_owned()));
        }
    }
    let cherry = git(worktree, &["cherry", base, &ticket.head])?;
    if !cherry.lines().any(|line| line.starts_with('+')) {
        return Ok(Pick::Decided(format!("landed {base}")));
    }
    let picked_onto = git(worktree, &["rev-parse", "HEAD"])?;
    let picked = output(worktree, &["cherry-pick", "--allow-empty", &format!("{base}..{}", ticket.head)])?;
    if !picked.status.success() {
        let files = git(worktree, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default().lines().collect::<Vec<_>>().join(",");
        let dropped = if files.is_empty() && succeeds(worktree, &["diff", "--cached", "--quiet", "HEAD"]) { git(worktree, &["rev-parse", "-q", "--verify", "CHERRY_PICK_HEAD"]).ok() } else { None };
        if !succeeds(worktree, &["cherry-pick", "--abort"]) {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        }
        if let Some(commit) = dropped {
            return refuse(logs, name, "dropped", &dropped_refusal(base, &commit));
        }
        return Ok(Pick::Decided(format!("conflict {}", if files.is_empty() { "cherry-pick" } else { files.as_str() })));
    }
    let range = format!("{base}..{}", ticket.head);
    if git(worktree, &["rev-list", "--count", &range])? != git(worktree, &["rev-list", "--count", &format!("{picked_onto}..HEAD")])? {
        git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        return refuse(logs, name, "dropped", &dropped_refusal(base, &git(worktree, &["rev-list", "--reverse", &range])?.replace('\n', " ")));
    }
    match stale_revert::reverted(worktree, &picked_onto, "HEAD", &ticket.head) {
        Ok(found) if found.is_empty() => Ok(Pick::Included),
        Ok(found) => {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            refuse(logs, name, "stale-revert", &stale_revert::refusal(&found))
        }
        Err(error) => {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            Ok(Pick::Undecided(format!("cannot check whether {} reverts recent main: {error}", ticket.branch)))
        }
    }
}

pub fn build(worktree: &Path, base: &str, tickets: &[(PathBuf, Ticket)], limit: usize, logs: &Path) -> Result<Built, String> {
    git(worktree, &["reset", "-q", "--hard"])?;
    git(worktree, &["checkout", "-q", "-B", BRANCH, base])?;
    let mut picks = Vec::new();
    let mut included = 0;
    for (path, ticket) in tickets {
        if included >= limit {
            break;
        }
        let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("lane");
        let pick = pick(worktree, base, ticket, logs, name)?;
        included += usize::from(matches!(pick, Pick::Included));
        picks.push(pick);
    }
    Ok(Built { head: git(worktree, &["rev-parse", "HEAD"])?, picks })
}
