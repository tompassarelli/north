use super::gh_write::{self, GhWrite};
use super::issue_labels::gh;
use crate::hook_input::{HookInput, Verdict};
use crate::platform::{box_oracles, cleanup};

const OWNER: &str = "tompassarelli/";
struct Live {
    cwd: Option<String>,
}

trait World {
    fn repo(&mut self, named: Option<&str>) -> Option<String>;
    fn body(&mut self, repo: &str, number: u64) -> Option<String>;
}

impl World for Live {
    fn repo(&mut self, named: Option<&str>) -> Option<String> {
        match named {
            Some(repo) => Some(repo.to_owned()),
            None => gh(self.cwd.as_deref(), &["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"]).map(|out| out.trim().to_owned()),
        }
    }

    fn body(&mut self, repo: &str, number: u64) -> Option<String> {
        gh(None, &["issue", "view", &number.to_string(), "-R", repo, "--json", "body", "-q", ".body"])
    }
}

pub fn close(input: &HookInput) -> Verdict {
    refuse_close(input, &mut Live { cwd: input.cwd().map(str::to_owned) })
}

fn refuse_close(input: &HookInput, world: &mut impl World) -> Verdict {
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    let mut edited: Vec<String> = Vec::new();
    for write in gh_write::writes(&text.lossy(), input.cwd()) {
        let closes = write.closes_issue.as_deref().map(|target| target.trim_start_matches('#').rsplit('/').next().unwrap_or_default().to_owned());
        if let Some(reason) = closes.as_ref().filter(|number| !edited.contains(number)).and_then(|_| unticked(&write, world)) {
            return Verdict::Deny(reason);
        }
        if let Some((group, number)) = &write.edits
            && group == "issue"
        {
            edited.push(number.clone());
        }
    }
    Verdict::Allow
}

fn unticked(write: &GhWrite, world: &mut impl World) -> Option<String> {
    let target = write.closes_issue.as_deref()?;
    if write.reason.as_deref().is_some_and(|reason| reason.replace('_', " ").eq_ignore_ascii_case("not planned")) {
        return None;
    }
    let number: u64 = target.rsplit('/').next()?.trim_start_matches('#').parse().ok()?;
    let repo = world.repo(write.repo.as_deref())?;
    if !repo.to_lowercase().starts_with(OWNER) || !cleanup::covers(&repo) {
        return None;
    }
    let body = world.body(&repo, number)?;
    unproven(&repo, number, &body)
}

fn unproven(repo: &str, number: u64, body: &str) -> Option<String> {
    let open = box_oracles::unproven(body);
    (!open.is_empty()).then(|| {
        format!(
            "No. {repo}#{number} closes only when each ticked box's oracle has run and passed, or the box is split with an owner. Untick each box below, write its oracle as `Done when:` followed by the command in backticks, and run `north task check --done-when {repo}#{number}`: it runs each oracle on main and ticks the box with its passing Evidence. A box this change does not finish says `split to REPO#N`, naming the issue that owns the rest. Unproven:\n{}",
            open.join("\n")
        )
    })
}

#[cfg(test)]
#[path = "issue_close_tests.rs"]
mod tests;
