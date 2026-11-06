use std::path::PathBuf;

use north_base::foundation::step::{Refused, Step, cancelled, refuse};

use super::name::release_tag;
use crate::session::identity::Identity;
use crate::session::output_log::claim_output_log;
use crate::shell::Shell;
use crate::shell::origin::{Hooks, Push};

pub struct DeleteRequest<'a> {
    pub tag: &'a str,
    pub dry_run: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Custody {
    DefaultBranch(String),
    Mirror(PathBuf),
}

pub fn custody(default_branch: String, on_default: bool, holding_mirror: Option<PathBuf>) -> Option<Custody> {
    if on_default { Some(Custody::DefaultBranch(default_branch)) } else { holding_mirror.map(Custody::Mirror) }
}

pub fn delete_tag(request: &DeleteRequest, shell: &Shell) -> Step {
    let claims = claim_output_log()?;
    let tag = request.tag;
    if release_tag(tag) {
        return Err(refuse(format!("safe-push: tag '{tag}' is a release (v*) — releases are never deleted; NOT deleted.")));
    }
    let dir = std::env::current_dir().map_err(|_| Refused(1))?;
    let identity = Identity::capture(&dir)?;
    let tag_ref = format!("refs/tags/{tag}");
    if !shell.git.valid_reference(&tag_ref) {
        return Err(refuse(format!("safe-push: '{tag}' is not a valid tag name")));
    }
    let peeled_ref = format!("{tag_ref}^{{}}");
    let listing = shell.origin.listing(&identity.origin_url, &[&tag_ref, &peeled_ref])?;
    let (tag_oid, commit) = origin_tag(&listing, &tag_ref).ok_or_else(|| refuse(format!("safe-push: origin has no tag '{tag}'")))?;
    let custody = observe_custody(shell, &identity.origin_url, &commit)?
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
    let refspec = format!(":{tag_ref}");
    let pushed = shell.origin.push(&Push { source: None, url: &identity.origin_url, refspec: &refspec, lease: Some(&lease), hooks: Hooks::Skip, claimed: &claims.list });
    if pushed == Err(Refused(127)) {
        return pushed;
    }
    cancelled()?;
    if pushed.is_err() {
        return Err(refuse(format!("safe-push: the push deleting tag '{tag}' failed")));
    }
    println!("safe-push: ✓ deleted tag {tag} from origin; {held}");
    Ok(())
}

fn origin_tag(listing: &str, tag_ref: &str) -> Option<(String, String)> {
    let peeled_ref = format!("{tag_ref}^{{}}");
    let (mut direct, mut peeled) = (None, None);
    for line in listing.lines() {
        match line.split_once('\t') {
            Some((oid, name)) if name == tag_ref => direct = Some(oid.to_owned()),
            Some((oid, name)) if name == peeled_ref => peeled = Some(oid.to_owned()),
            _ => {}
        }
    }
    direct.map(|oid| {
        let commit = peeled.unwrap_or_else(|| oid.clone());
        (oid, commit)
    })
}

fn observe_custody(shell: &Shell, origin_url: &str, commit: &str) -> Step<Option<Custody>> {
    let git = &*shell.git;
    let branch = git.default_branch();
    let scratch = format!("refs/safe-push-delete/{}", std::process::id());
    let fetched = shell.origin.fetch_scratch(origin_url, &format!("+refs/heads/{branch}:{scratch}"));
    cancelled()?;
    let on_default = fetched && git.has_commit(commit) && git.is_ancestor(commit, &scratch);
    if fetched {
        git.delete_ref(&scratch);
    }
    let mirror = shell.settings.mirror(origin_url);
    let holding_mirror = (!on_default && shell.files.is_directory(&mirror) && git.mirror_holds(&mirror, commit, &[])).then_some(mirror);
    Ok(custody(branch, on_default, holding_mirror))
}
