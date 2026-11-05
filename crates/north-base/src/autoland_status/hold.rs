use super::{commit_status, text};
use crate::platform::process;
use std::process::Command;
use std::time::{Duration, Instant};
use crate::foundation::clock::Timestamp;
use serde_json::Value;

const QUEUE_PREFIX: &str = "claude/land-";
const HOOK_BUDGET: Duration = Duration::from_secs(8);
const PENDING_SETTLE_MS: i64 = 24 * 3_600_000;
const UNKNOWN_SETTLE_MS: i64 = 4 * 3_600_000;
const DISPATCHED: [&str; 10] = ["run", "list", "--workflow", "autoland.yml", "--event", "workflow_dispatch", "--limit", "100", "--json", "databaseId,displayTitle,event,status,conclusion,url,createdAt"];
const OWN: [&str; 10] = ["run", "list", "--workflow", "autoland.yml", "--branch", "", "--limit", "20", "--json", "status,createdAt,headBranch,url"];
const WORKFLOWS: [&str; 5] = ["api", "repos/{owner}/{repo}/actions/workflows", "--paginate", "--jq", ".workflows[].path"];

fn unreadable(branch: &str) -> Option<String> {
    held(branch, "its Autoland runs could not be read")
}

fn held(branch: &str, why: &str) -> Option<String> {
    Some(format!("{branch}: {why}; deleting it would drop the lane unlanded, so wait for Autoland to land or hand it back"))
}

fn started_ms(run: &Value) -> Option<i64> {
    text(run, "createdAt").and_then(Timestamp::parse_rfc3339).map(Timestamp::unix_millis)
}

fn unfinished(run: &Value) -> bool {
    text(run, "status") != Some("completed")
}

pub fn deletion_hold(branch: &str, tip: &str, now_ms: i64, mut gh: impl FnMut(&[&str]) -> Option<String>) -> Option<String> {
    if !branch.starts_with(QUEUE_PREFIX) {
        return None;
    }
    let Some(outcome) = commit_status(tip, &mut gh) else {
        return unreadable(branch);
    };
    let state = outcome.split('\t').next().unwrap_or("").trim();
    if matches!(state, "success" | "failure" | "error") {
        return None;
    }
    let Some(workflows) = gh(&WORKFLOWS) else {
        return unreadable(branch);
    };
    if !workflows.lines().any(|path| path.trim().ends_with("/autoland.yml")) {
        return None;
    }
    let Some(dispatched) = gh(&DISPATCHED).and_then(|payload| serde_json::from_str::<Value>(&payload).ok()) else {
        return unreadable(branch);
    };
    let mut own = OWN;
    own[5] = branch;
    let Some(pushes) = gh(&own).and_then(|payload| serde_json::from_str::<Value>(&payload).ok()) else {
        return unreadable(branch);
    };
    let (Some(dispatched), Some(pushes)) = (dispatched.as_array(), pushes.as_array()) else {
        return unreadable(branch);
    };
    let suffix = format!(" {branch} {tip}");
    let title = |run: &Value| text(run, "displayTitle").unwrap_or_default().to_owned();
    let tests: Vec<&Value> = dispatched.iter().filter(|run| title(run).starts_with("Autoland test ") && title(run).ends_with(&suffix)).collect();
    let pushes: Vec<&Value> = pushes.iter().filter(|run| text(run, "headBranch") == Some(branch)).collect();
    let shared = dispatched.iter().filter(|run| title(run) == "Autoland train" || title(run).starts_with("Autoland plan"));
    if let Some(live) = tests.iter().chain(&pushes).copied().chain(shared).find(|run| unfinished(run)) {
        return held(branch, &format!("an Autoland run {} is {}", text(live, "url").unwrap_or("(no url)"), text(live, "status").unwrap_or("unfinished")));
    }
    let newest = tests.iter().chain(&pushes).filter_map(|run| started_ms(run)).max()?;
    let settle = if state == "pending" { PENDING_SETTLE_MS } else { UNKNOWN_SETTLE_MS };
    (now_ms - newest < settle).then(|| format!("{branch}: its newest Autoland run started under {} hours ago and Autoland has not landed or handed it back; deleting it would drop the lane unlanded, so wait for Autoland to land or hand it back", settle / 3_600_000))
}

fn within(deadline: Instant, program: &str, arguments: &[&str], repository: Option<&str>) -> Option<String> {
    let mut command = Command::new(program);
    command.args(arguments);
    if let Some(repository) = repository {
        command.env("GH_REPO", repository);
    }
    let left = deadline.checked_duration_since(Instant::now()).filter(|left| !left.is_zero())?;
    process::run_command_within(command, None, left)
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn repository_slug(url: &str) -> Option<String> {
    let tail = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let mut parts = tail.rsplit(['/', ':']);
    let (repo, owner) = (parts.next()?, parts.next()?);
    (!repo.is_empty() && !owner.is_empty()).then(|| format!("{owner}/{repo}"))
}

pub fn hook_deadline() -> Instant {
    Instant::now() + HOOK_BUDGET
}

pub fn origin_branches_hold(deadline: Instant, directory: &str, remote: &str, branches: &[String]) -> Option<String> {
    branches.iter().find_map(|branch| origin_branch_hold(deadline, directory, remote, branch))
}

fn origin_branch_hold(deadline: Instant, directory: &str, remote: &str, branch: &str) -> Option<String> {
    if !branch.starts_with(QUEUE_PREFIX) {
        return None;
    }
    let git = |arguments: &[&str]| within(deadline, "git", arguments, None);
    let Some(listed) = git(&["-C", directory, "ls-remote", remote, &format!("refs/heads/{branch}")]) else {
        return held(branch, "origin's tip could not be read in time, so a queued Autoland run cannot be ruled out");
    };
    let tip = listed.split_whitespace().next()?;
    let url = git(&["-C", directory, "remote", "get-url", "--push", remote]).unwrap_or_else(|| remote.to_owned());
    let slug = repository_slug(&url);
    deletion_hold(branch, tip, Timestamp::now().unix_millis(), |arguments| within(deadline, "gh", arguments, slug.as_deref()))
}
