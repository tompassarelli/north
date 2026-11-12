use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_base::platform::finish_record::FinishRecords;
use serde::Deserialize;

use crate::issue_body::{self, Edit, Issue};
use crate::platform::process;

const SHORT_SHA: usize = 12;
const CLAIMS_LIMIT: Duration = Duration::from_secs(30);

pub struct Landed<'a> {
    pub repo: &'a Path,
    pub repository: Option<String>,
    pub session: Option<&'a str>,
    pub head: &'a str,
    pub tip: &'a str,
    pub main: &'a str,
    pub check: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
pub struct Claim {
    pub owner: String,
    pub item: String,
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(SHORT_SHA)]
}

fn git(repo: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn tree(repo: &Path, commit: &str) -> Option<String> {
    git(repo, &["rev-parse", "--verify", "--quiet", &format!("{commit}^{{tree}}")]).ok()
}

pub fn items(message: &str) -> Vec<Issue> {
    let mut alone = Vec::new();
    let mut named: Option<(&str, bool)> = None;
    for line in message.lines() {
        if let Some(value) = line.strip_prefix("Item:") {
            alone.extend(named.take().filter(|(_, boxed)| !boxed).map(|(item, _)| item));
            named = value.split_whitespace().next().map(|item| (item, false));
        } else if line.starts_with("Box:")
            && let Some(named) = named.as_mut()
        {
            named.1 = true;
        }
    }
    alone.extend(named.filter(|(_, boxed)| !boxed).map(|(item, _)| item));
    alone.into_iter().filter_map(|item| Issue::parse(item).ok()).collect()
}

pub fn passed(records: &FinishRecords, session: &str, repo: &Path, head: &str) -> Option<String> {
    let finished = records.read(session).filter(|finished| finished.passed && !finished.head.is_empty())?;
    (tree(repo, &finished.head)? == tree(repo, head)?).then_some(finished.command)
}

pub fn claims() -> Result<Vec<Claim>, String> {
    let north = std::env::current_exe().map_err(|error| format!("cannot find the north binary: {error}"))?;
    let output = process::run_with_input_within(&north.to_string_lossy(), &["threads", "list", "--json"], None, CLAIMS_LIMIT).ok_or_else(|| format!("north threads list timed out after {} s", CLAIMS_LIMIT.as_secs()))?;
    if !output.status.success() {
        return Err(format!("north threads list: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("north threads list --json: {error}"))
}

pub fn queue(queue_directory: &Path, landed: &Landed, claims: &dyn Fn() -> Result<Vec<Claim>, String>) -> Vec<String> {
    let message = match git(landed.repo, &["log", "-1", "--format=%B", landed.tip]) {
        Ok(message) => message,
        Err(error) => return vec![format!("no issue closed: {error}")],
    };
    let items = items(&message);
    if items.is_empty() {
        return Vec::new();
    }
    if let Err(error) = git(landed.repo, &["merge-base", "--is-ancestor", landed.tip, landed.main]) {
        return vec![format!("no issue closed: {} is not an ancestor of {}: {error}", short(landed.tip), landed.main)];
    }
    let Some(session) = landed.session else {
        return vec!["no issue closed: no session queued the lane, so no claim shows it held an issue".to_owned()];
    };
    let claims = match claims() {
        Ok(claims) => claims,
        Err(error) => return vec![format!("no issue closed: the claims could not be read: {error}")],
    };
    let same_tree = tree(landed.repo, landed.head).is_some_and(|lane| Some(lane) == tree(landed.repo, landed.tip));
    let check = landed.check.filter(|_| same_tree);
    items
        .into_iter()
        .map(|issue| {
            if !landed.repository.as_deref().is_some_and(|repository| repository.eq_ignore_ascii_case(&issue.repo)) {
                return format!("{issue} not closed: the lane landed in {}, not in the issue's repository", landed.repository.as_deref().unwrap_or("no GitHub repository"));
            }
            if !claims.iter().any(|claim| claim.owner == session && claim.item == issue.item()) {
                return format!("{issue} not closed: session {session} holds no claim on it");
            }
            let edit = Edit::Close { sha: landed.tip.to_owned(), check: check.map(str::to_owned) };
            match issue_body::submit(queue_directory, &issue, &edit) {
                Ok(_) if check.is_none() && landed.check.is_some() => format!("queued {issue} to wait: its check passed on the lane's tree and {} landed with another", short(landed.tip)),
                Ok(_) => format!("queued the close of {issue} by its check, landed in {}", short(landed.tip)),
                Err(error) => format!("cannot queue the close of {issue}: {error}"),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "leaf_tests.rs"]
mod tests;
