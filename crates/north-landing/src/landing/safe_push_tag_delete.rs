use std::path::{Path, PathBuf};
use std::process::Command;

use super::safe_push_plan::default_branch;
use super::safe_push_state::{Identity, Refused, Step, cancelled, git_quiet, git_read, refuse};
use super::safe_push_tag::claim_output_log;

pub struct DeleteRequest<'a> {
    pub tag: &'a str,
    pub dry_run: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Custody {
    DefaultBranch(String),
    Mirror(PathBuf),
}

pub fn delete_tag(request: &DeleteRequest) -> Step {
    let claims = claim_output_log()?;
    let tag = request.tag;
    if tag.starts_with('v') {
        return Err(refuse(format!("safe-push: tag '{tag}' is a release (v*) — releases are never deleted; NOT deleted.")));
    }
    let dir = std::env::current_dir().map_err(|_| Refused(1))?;
    let identity = Identity::capture(&dir)?;
    let tag_ref = format!("refs/tags/{tag}");
    if git_quiet(&dir, &["check-ref-format", &tag_ref]).is_none() {
        return Err(refuse(format!("safe-push: '{tag}' is not a valid tag name")));
    }
    let (tag_oid, commit) = origin_tag(&dir, &identity.origin_url, &tag_ref)?
        .ok_or_else(|| refuse(format!("safe-push: origin has no tag '{tag}'")))?;
    let custody = custody(&dir, &identity.origin_url, &commit)?
        .ok_or_else(|| refuse(format!("safe-push: commit {commit} of tag '{tag}' is neither on origin's default branch nor held in the lane-gc mirror — refusing; NOT deleted.")))?;
    cancelled()?;
    let held = match &custody {
        Custody::DefaultBranch(branch) => format!("commit {commit} is on origin/{branch}"),
        Custody::Mirror(mirror) => format!("commit {commit} is held by {}", mirror.display()),
    };
    if request.dry_run {
        println!("safe-push: ✓ clean (--dry-run) — would delete tag {tag} from origin; {held}");
        return Ok(());
    }
    let lease = format!("--force-with-lease={tag_ref}:{tag_oid}");
    let status = Command::new("git")
        .current_dir(&dir)
        .env("SAFE_PUSH_ACTIVE", "1")
        .env("SAFE_PUSH_OUTPUT_CLAIMED", &claims.list)
        .args(["push", "--no-follow-tags", "--no-verify", &lease, &identity.origin_url, &format!(":{tag_ref}")])
        .status()
        .map_err(|_| Refused(127))?;
    cancelled()?;
    if !status.success() {
        return Err(refuse(format!("safe-push: the push deleting tag '{tag}' failed")));
    }
    println!("safe-push: ✓ deleted tag {tag} from origin; {held}");
    Ok(())
}

fn origin_tag(dir: &Path, origin_url: &str, tag_ref: &str) -> Step<Option<(String, String)>> {
    let peeled_ref = format!("{tag_ref}^{{}}");
    let listing = git_read(dir, &["ls-remote", origin_url, tag_ref, &peeled_ref])?;
    let (mut direct, mut peeled) = (None, None);
    for line in listing.lines() {
        match line.split_once('\t') {
            Some((oid, name)) if name == tag_ref => direct = Some(oid.to_owned()),
            Some((oid, name)) if name == peeled_ref => peeled = Some(oid.to_owned()),
            _ => {}
        }
    }
    Ok(direct.map(|oid| {
        let commit = peeled.unwrap_or_else(|| oid.clone());
        (oid, commit)
    }))
}

fn custody(dir: &Path, origin_url: &str, commit: &str) -> Step<Option<Custody>> {
    let branch = default_branch(dir);
    let scratch = format!("refs/safe-push-delete/{}", std::process::id());
    let fetched = git_quiet(dir, &["fetch", "-q", "--no-tags", "--no-write-fetch-head", origin_url, &format!("+refs/heads/{branch}:{scratch}")]).is_some();
    cancelled()?;
    let on_default = fetched && git_quiet(dir, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_some() && Command::new("git")
        .current_dir(dir)
        .args(["merge-base", "--is-ancestor", commit, &scratch])
        .status()
        .is_ok_and(|status| status.success());
    if fetched {
        let _ = git_quiet(dir, &["update-ref", "-d", &scratch]);
    }
    if on_default {
        return Ok(Some(Custody::DefaultBranch(branch)));
    }
    Ok(mirror_path(origin_url).filter(|mirror| mirror_holds(mirror, commit)).map(Custody::Mirror))
}

pub fn mirror_path(origin_url: &str) -> Option<PathBuf> {
    let name = origin_url.trim_end_matches('/').rsplit(['/', ':']).next()?.trim_end_matches(".git");
    if name.is_empty() {
        return None;
    }
    let root = std::env::var_os("LANE_GC_MIRRORS")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/state/agents/lane-gc/mirrors")))?;
    Some(root.join(format!("{name}.git")))
}

fn mirror_holds(mirror: &Path, commit: &str) -> bool {
    if !mirror.is_dir() {
        return false;
    }
    let held = Command::new("git").arg("--git-dir").arg(mirror).args(["for-each-ref", "--count=1", "--contains", commit, "--format=%(refname)"]).output();
    held.is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
}

#[cfg(test)]
#[path = "safe_push_tag_delete_tests.rs"]
mod tests;
