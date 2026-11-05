use super::gh_write::{self, GhWrite};
use super::issue_filing::Ref;
use super::issue_labels::gh;
use crate::hook::{HookInput, Verdict};
use crate::platform::cleanup;

const OWNER: &str = "tompassarelli/";

pub(super) fn append(new: &Ref) -> Result<bool, String> {
    let (repo, number) = (&new.0, new.1.to_string());
    if !cleanup::covers(repo) {
        return Ok(false);
    }
    let issue = gh(None, &["api", &format!("repos/{repo}/issues/{number}"), "--jq", "{body: (.body // \"\"), labels: [.labels[].name]}"])
        .and_then(|out| serde_json::from_str::<serde_json::Value>(&out).ok())
        .ok_or("the new issue could not be read")?;
    let Some(body) = amended(&issue) else {
        return Ok(false);
    };
    let file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    std::fs::write(file.path(), body).map_err(|error| error.to_string())?;
    let path = file.path().to_string_lossy();
    gh(None, &["issue", "edit", &number, "-R", repo, "--body-file", &path]).map(|_| true).ok_or_else(|| "gh issue edit failed".to_owned())
}

fn amended(issue: &serde_json::Value) -> Option<String> {
    let labels: Vec<String> = issue["labels"].as_array().into_iter().flatten().filter_map(|label| label.as_str().map(str::to_owned)).collect();
    cleanup::applies(&labels).then(|| cleanup::appended(issue["body"].as_str().unwrap_or_default())).flatten()
}

struct Live {
    cwd: Option<String>,
}

impl Live {
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
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    refuse_close(input, &mut Live { cwd: input.cwd().map(str::to_owned) })
}

fn refuse_close(input: &HookInput, world: &mut Live) -> Verdict {
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    for write in gh_write::writes(&text.lossy(), input.cwd()) {
        if let Some(reason) = unticked(&write, world) {
            return Verdict::Deny(reason);
        }
    }
    Verdict::Allow
}

fn unticked(write: &GhWrite, world: &mut Live) -> Option<String> {
    let target = write.closes_issue.as_deref()?;
    let number: u64 = target.rsplit('/').next()?.trim_start_matches('#').parse().ok()?;
    let repo = world.repo(write.repo.as_deref())?;
    if !repo.to_lowercase().starts_with(OWNER) || !cleanup::covers(&repo) {
        return None;
    }
    let body = world.body(&repo, number)?;
    let open = cleanup::unticked(&body);
    (!open.is_empty()).then(|| {
        format!(
            "No. {repo}#{number} still has {} unticked Clean-up box(es). Delete the tests that pin no guarantee a person would notice, the scaffolding and the follow-up notes this change left, then tick each box with what you deleted (`gh issue view {number} -R {repo} --json body -q .body > /tmp/{number}.md`, edit, `gh issue edit {number} -R {repo} --body-file /tmp/{number}.md`) and close again. Unticked:\n{}",
            open.len(),
            open.join("\n")
        )
    })
}
