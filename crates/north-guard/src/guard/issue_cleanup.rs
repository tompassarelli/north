use super::gh_write::{self, GhWrite};
use super::issue_filing::Ref;
use super::issue_labels::gh;
use crate::hook_input::{HookInput, Verdict};
use crate::platform::{box_oracles, cleanup};

const OWNER: &str = "tompassarelli/";
const READ_TRIES: u32 = 3;
const READ_PAUSE: std::time::Duration = std::time::Duration::from_secs(1);

pub(super) fn append(new: &Ref) -> Result<bool, String> {
    let (repo, number) = (&new.0, new.1.to_string());
    if !cleanup::covers(repo) {
        return Ok(false);
    }
    let issue = created(repo, &number)?;
    let Some(body) = amended(&issue) else {
        return Ok(false);
    };
    let file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    std::fs::write(file.path(), body).map_err(|error| error.to_string())?;
    let path = file.path().to_string_lossy();
    gh(None, &["issue", "edit", &number, "-R", repo, "--body-file", &path]).map(|_| true).ok_or_else(|| "gh issue edit failed".to_owned())
}

/// The issue just filed; GitHub can answer 404 for a second or two after the create returns its URL.
fn created(repo: &str, number: &str) -> Result<serde_json::Value, String> {
    let mut last = String::new();
    for attempt in 0..READ_TRIES {
        if attempt > 0 {
            std::thread::sleep(READ_PAUSE);
        }
        let out = std::process::Command::new("gh")
            .args(["api", &format!("repos/{repo}/issues/{number}"), "--jq", "{body: (.body // \"\"), labels: [.labels[].name]}"])
            .output()
            .map_err(|error| error.to_string())?;
        if let Some(issue) = out.status.success().then(|| serde_json::from_slice(&out.stdout).ok()).flatten() {
            return Ok(issue);
        }
        last = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    }
    Err(format!("the new issue could not be read after {READ_TRIES} tries: {last}"))
}

fn amended(issue: &serde_json::Value) -> Option<String> {
    let labels: Vec<String> = issue["labels"].as_array().into_iter().flatten().filter_map(|label| label.as_str().map(str::to_owned)).collect();
    cleanup::applies(&labels).then(|| cleanup::appended(issue["body"].as_str().unwrap_or_default())).flatten()
}

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
    let open = cleanup::unticked(&body);
    if open.is_empty() {
        return unproven(&repo, number, &body);
    }
    Some(format!(
        "No. {repo}#{number} still has {} unticked Clean-up box(es). Delete the tests that pin no guarantee a person would notice, the scaffolding and the follow-up notes this change left, then tick each box with what you deleted (`gh issue view {number} -R {repo} --json body -q .body > /tmp/{number}.md`, edit, `gh issue edit {number} -R {repo} --body-file /tmp/{number}.md`) and close again. Unticked:\n{}",
        open.len(),
        open.join("\n")
    ))
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
#[path = "issue_cleanup_tests.rs"]
mod tests;
