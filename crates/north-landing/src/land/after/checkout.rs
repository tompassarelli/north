//! Fast-forward the main checkout a lander serves past the commit it landed. Every process on the host that
//! reads a project's working tree reads that checkout: placement reads south's `agents/orchestration.toml` for
//! the resident cap, `agents sync` projects the agent policy from it, and a lane is cut from it. A landing that
//! moved `origin/main` without moving it leaves the host on the old file until someone fast-forwards by hand.
//!
//! The merge is best effort, never a reason to hold a landing back: a checkout on another branch, with local
//! commits or with uncommitted changes is left exactly where it is, and the reason is printed.

use std::path::Path;

use crate::land::candidate::{git, output};
use crate::land::lander::Lander;

const SHORT: usize = 12;

/// Fast-forwards the checkout `lander` serves to the landed `head`, printing the move or why it was left where it was.
pub fn follow(lander: &Lander, head: &str) {
    match fast_forward(&lander.repo, head) {
        Ok(Some(line)) => println!("{line}"),
        Ok(None) => {}
        Err(reason) => println!("north land: {reason}"),
    }
}

/// Fast-forwards `repo`'s main to `head` when it is a clean checkout of `main` behind it: the line to print when it
/// moved, `None` when it already held `head`, or why it was left where it was.
pub fn fast_forward(repo: &Path, head: &str) -> Result<Option<String>, String> {
    let at = git(repo, &["rev-parse", "--verify", "HEAD"])?;
    if at == head {
        return Ok(None);
    }
    let branch = git(repo, &["symbolic-ref", "-q", "--short", "HEAD"]).unwrap_or_default();
    if branch != "main" {
        let on = if branch.is_empty() { "no branch".to_owned() } else { format!("branch {branch}") };
        return Err(format!("the served checkout {} is on {on}, not main, so it was left where it is", repo.display()));
    }
    let dirty = git(repo, &["status", "--porcelain", "--untracked-files=no"])?;
    if !dirty.is_empty() {
        return Err(format!("the served checkout {} has uncommitted changes, so it was left where it is", repo.display()));
    }
    if !ancestor(repo, &at, head) {
        return Err(format!("the served checkout {} at {} is not behind the landed {} and was left where it is", repo.display(), short(&at), short(head)));
    }
    git(repo, &["merge", "-q", "--ff-only", head])?;
    Ok(Some(format!("north land: fast-forwarded the served checkout {} to {}", repo.display(), short(head))))
}

fn ancestor(repo: &Path, ancestor: &str, descendant: &str) -> bool {
    output(repo, &["merge-base", "--is-ancestor", ancestor, descendant]).is_ok_and(|output| output.status.success())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(SHORT)]
}
