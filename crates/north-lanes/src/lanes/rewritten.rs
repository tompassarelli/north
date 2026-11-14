use std::path::Path;

use super::repo::{find_container, git_checked, git_out};

pub(super) enum Followed {
    Moved(String),
    Already,
    Plan(String),
}

pub(super) fn follow(start: &Path, dry_run: bool) -> Result<Followed, String> {
    let container = find_container(start).ok_or_else(|| format!("no container found above {} (no <dir>/main/.git)", start.display()))?;
    let main_dir = container.join("main");
    if git_out(&main_dir, &["symbolic-ref", "--short", "HEAD"]).as_deref() != Some("main") {
        return Err(format!("{} is not on branch main", main_dir.display()));
    }
    let status = git_out(&main_dir, &["--no-optional-locks", "status", "--porcelain"]).ok_or_else(|| format!("could not read the status of {}", main_dir.display()))?;
    if !status.is_empty() {
        return Err(format!("{} has uncommitted or untracked changes; run `north lane rescue` without --rewritten first", main_dir.display()));
    }
    git_checked(&main_dir, &["fetch", "-q", "origin", "main"])?;
    let read = |name: &str| git_out(&main_dir, &["rev-parse", "--verify", name]).ok_or_else(|| format!("could not read {name}"));
    let (head, target) = (read("HEAD")?, read("refs/remotes/origin/main")?);
    if head == target {
        return Ok(Followed::Already);
    }
    let (tree, target_tree) = (read("HEAD^{tree}")?, read("refs/remotes/origin/main^{tree}")?);
    if tree != target_tree {
        return Err(format!("HEAD {} holds tree {}, origin/main {} holds tree {}; only a rewrite that keeps the tree is followed", short(&head), short(&tree), short(&target), short(&target_tree)));
    }
    let line = format!("main {} -> origin/main {} (same tree {})", short(&head), short(&target), short(&tree));
    if dry_run {
        return Ok(Followed::Plan(line));
    }
    git_checked(&main_dir, &["update-ref", "-m", "north lane rescue --rewritten", "refs/heads/main", &target, &head])?;
    Ok(Followed::Moved(line))
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}
