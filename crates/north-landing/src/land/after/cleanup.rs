use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::candidate::{git, output};
use super::super::lander::Lander;
use super::super::queue::{self, Ticket};

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Removed,
    Kept(String),
}

fn on_main(repo: &Path, landed: &str, commit: &str) -> bool {
    git(repo, &["cherry", landed, commit]).is_ok_and(|cherry| !cherry.lines().any(|line| line.starts_with('+')))
}

fn registered_branch(lander: &Lander, root: &Path) -> Option<String> {
    let listing = git(&lander.repo, &["worktree", "list", "--porcelain"]).ok()?;
    listing.split("\n\n").find_map(|entry| {
        let mut path = None;
        let mut branch = None;
        let mut blocked = false;
        for line in entry.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(value));
            } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
                branch = Some(value.to_owned());
            } else if line.starts_with("locked") || line.starts_with("prunable") {
                blocked = true;
            }
        }
        (path.as_deref() == Some(root) && !blocked).then_some(branch).flatten()
    })
}

fn lane_layout(lander: &Lander, root: &Path) -> bool {
    let Some(container) = root.parent() else { return false };
    container.file_name().is_some_and(|name| name == "worktrees") && container.parent() == lander.repo.parent() && root != lander.repo
}

fn identity(path: &Path) -> Option<(u64, u64)> {
    fs::metadata(path).ok().map(|metadata| (metadata.dev(), metadata.ino()))
}

/// Whether the directory at `cwd` is `lane` or below it, compared by device and inode while
/// climbing `..`. A process sandboxed in its own mount and PID namespaces sees the lane under its
/// sandbox path, so the text of its `/proc/<pid>/cwd` link need not name the lane at all.
fn inside(cwd: &Path, lane: (u64, u64)) -> bool {
    let mut directory = cwd.to_path_buf();
    let mut below = None;
    while let Some(here) = identity(&directory) {
        if here == lane {
            return true;
        }
        if below == Some(here) {
            return false;
        }
        below = Some(here);
        directory.push("..");
    }
    false
}

fn occupant(root: &Path) -> Option<u32> {
    let lane = identity(root)?;
    fs::read_dir("/proc").ok()?.flatten().find_map(|entry| {
        let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
        inside(&entry.path().join("cwd"), lane).then_some(pid)
    })
}

fn delete_merged_remote_branch(lander: &Lander, ticket: &Ticket, landed: &str) {
    let reference = format!("refs/heads/{}", ticket.branch);
    let Ok(listing) = git(&lander.repo, &["ls-remote", "origin", &reference]) else { return };
    let Some(remote_oid) = listing.split_whitespace().next().filter(|oid| !oid.is_empty()) else { return };
    let scratch = format!("refs/land-cleanup/{}", queue::slug(&ticket.branch));
    if git(&lander.repo, &["fetch", "-q", "--no-tags", "origin", &format!("+{reference}:{scratch}")]).is_err() {
        return;
    }
    let merged = on_main(&lander.repo, landed, &scratch);
    let _ = git(&lander.repo, &["update-ref", "-d", &scratch]);
    if !merged {
        println!("north land: origin {} holds commits that are not on main; left in place", ticket.branch);
        return;
    }
    let pushed = Command::new("git")
        .arg("-C")
        .arg(&lander.repo)
        .args(["push", "-q", "--no-verify", "--no-follow-tags", &format!("--force-with-lease={reference}:{remote_oid}"), "origin", &format!(":{reference}")])
        .env("SAFE_PUSH_ACTIVE", "1")
        .output();
    match pushed {
        Ok(result) if result.status.success() => println!("north land: origin {} is fully on main; deleted it", ticket.branch),
        _ => println!("north land: could not delete origin {}; left in place", ticket.branch),
    }
}

fn judge(lander: &Lander, ticket: &Ticket, landed: &str) -> Result<PathBuf, String> {
    let root = fs::canonicalize(&ticket.root).map_err(|_| "its worktree is not on this host".to_owned())?;
    if !lane_layout(lander, &root) {
        return Err("it is not a lane worktree of this repository".to_owned());
    }
    if registered_branch(lander, &root).as_deref() != Some(ticket.branch.as_str()) {
        return Err("its worktree is not checked out on the ticket's branch".to_owned());
    }
    let head = git(&root, &["rev-parse", "HEAD"])?;
    let branch_tip = git(&lander.repo, &["rev-parse", "--verify", &format!("refs/heads/{}^{{commit}}", ticket.branch)])?;
    if head != ticket.head || branch_tip != ticket.head {
        return Err("the lane moved since it was queued".to_owned());
    }
    if !on_main(&lander.repo, landed, &ticket.head) {
        return Err("its commits are not all on main".to_owned());
    }
    if ticket.keep_lane {
        return Err("--keep-lane".to_owned());
    }
    let status = output(&root, &["status", "--porcelain"])?;
    if !status.status.success() || !status.stdout.is_empty() {
        return Err("it has uncommitted or untracked files".to_owned());
    }
    if let Some(pid) = occupant(&root) {
        return Err(format!("process {pid} is running in it"));
    }
    Ok(root)
}

pub fn clean(lander: &Lander, ticket: &Ticket, landed: &str) -> Outcome {
    let judged = judge(lander, ticket, landed);
    if on_main(&lander.repo, landed, &ticket.head) {
        delete_merged_remote_branch(lander, ticket, landed);
    }
    let root = match judged {
        Ok(root) => root,
        Err(reason) => return Outcome::Kept(reason),
    };
    let Some(path) = root.to_str() else { return Outcome::Kept("its path is not UTF-8".to_owned()) };
    if let Err(error) = git(&lander.repo, &["worktree", "remove", path]) {
        return Outcome::Kept(error);
    }
    match git(&lander.repo, &["update-ref", "-d", &format!("refs/heads/{}", ticket.branch), &ticket.head]) {
        Ok(_) => Outcome::Removed,
        Err(error) => Outcome::Kept(format!("worktree removed but {error}")),
    }
}

pub fn report(lander: &Lander, ticket: &Ticket, landed: &str) {
    match clean(lander, ticket, landed) {
        Outcome::Removed => println!("north land: lane {} landed; removed its worktree {} and branch", ticket.branch, ticket.root.display()),
        Outcome::Kept(reason) => println!("north land: lane {} landed; kept {}: {reason}", ticket.branch, ticket.root.display()),
    }
}
