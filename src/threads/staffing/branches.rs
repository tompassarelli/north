//! The claude/* and *land-* branches on origin with a commit in the last two days, the commit
//! messages each holds ahead of the default branch, and the ones whose tip Autoland failed.
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::foundation::clock::Timestamp;
use crate::threads::catalog::CATALOG;
use crate::threads::verb::{Context, Failure};

pub(crate) const BRANCH_HOURS: i64 = 48;

const REFS: &str = "query=query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { refs(refPrefix: \"refs/heads/\", first: 100) { nodes { name target { ... on Commit { committedDate status { context(name: \"autoland\") { state description createdAt } } } } } } } }";

static COMMIT_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:item|closes|close|fixes|fix|refs|ref|part of|issue|for)\b").expect("commit ref pattern"));
static GONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Could not resolve head ref '([^']+)'").expect("gone ref pattern"));

/// A branch whose tip Autoland failed: why, when, and its commit messages.
pub(crate) struct Failed {
    pub reason: String,
    pub at: String,
    pub messages: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Lanes {
    pub open: BTreeMap<String, Vec<String>>,
    pub failed: BTreeMap<String, Failed>,
}

pub(crate) fn lanes(context: &Context, repo: &str, err: &mut dyn Write) -> Result<Lanes, Failure> {
    let (owner, name) = (format!("owner={}", CATALOG.owner), format!("name={repo}"));
    let reply = parsed(&context.gh(&["api", "graphql", "-f", REFS, "-F", &owner, "-F", &name])?)?;
    let cutoff = context.now.unix_seconds_f64() - (BRANCH_HOURS * 3600) as f64;
    let mut statuses: Vec<(String, Value)> = Vec::new();
    for node in reply["data"]["repository"]["refs"]["nodes"].as_array().ok_or_else(unreadable)? {
        let branch = node["name"].as_str().ok_or_else(unreadable)?;
        let committed = node["target"]["committedDate"].as_str().and_then(|at| Timestamp::parse(at).ok()).map_or(0.0, Timestamp::unix_seconds_f64);
        if (branch.starts_with("claude/") || branch.contains("land-")) && committed >= cutoff {
            statuses.push((branch.to_owned(), node["target"]["status"]["context"].clone()));
        }
    }
    let failed: BTreeMap<&str, (String, String)> = statuses
        .iter()
        .filter(|(_, status)| status["state"] == "FAILURE")
        .map(|(branch, status)| {
            let text = |key: &str| status[key].as_str().filter(|text| !text.is_empty()).map(str::to_owned);
            (branch.as_str(), (text("description").unwrap_or_else(|| "no reason given".into()), text("createdAt").unwrap_or_default()))
        })
        .collect();
    let mut names: Vec<&str> = statuses.iter().map(|(branch, _)| branch.as_str()).collect();
    let base = loop {
        if names.is_empty() {
            return Ok(Lanes::default());
        }
        let fields: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(index, branch)| format!("b{index}: compare(headRef: {}) {{ commits(first: 100) {{ nodes {{ message }} }} }}", crate::platform::pyjson::dumps(&(*branch).into())))
            .collect();
        let query = format!("query=query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ defaultBranchRef {{ {} }} }} }}", fields.join(" "));
        match context.gh(&["api", "graphql", "-f", &query, "-F", &owner, "-F", &name]) {
            Ok(reply) => break parsed(&reply)?["data"]["repository"]["defaultBranchRef"].clone(),
            Err(Failure::Refused(why)) => {
                let mut gone: Vec<&str> = GONE.captures_iter(&why).filter_map(|found| names.iter().copied().find(|branch| *branch == &found[1])).collect();
                if gone.is_empty() {
                    return Err(Failure::Refused(why));
                }
                gone.sort_unstable();
                gone.dedup();
                let _ = writeln!(err, "NorthThreads: skipped deleted {repo} branches {}", gone.join(", "));
                names.retain(|branch| !gone.contains(branch));
            }
            Err(usage) => return Err(usage),
        }
    };
    let mut lanes = Lanes::default();
    for (index, branch) in names.into_iter().enumerate() {
        let commits = base[format!("b{index}")]["commits"]["nodes"].as_array().cloned().unwrap_or_default();
        let messages: Vec<String> = commits.iter().map(|commit| commit["message"].as_str().unwrap_or_default().to_owned()).collect();
        if let Some((reason, at)) = failed.get(branch) {
            lanes.failed.insert(branch.to_owned(), Failed { reason: reason.clone(), at: at.clone(), messages });
        } else {
            lanes.open.insert(branch.to_owned(), messages);
        }
    }
    Ok(lanes)
}

fn unreadable() -> Failure {
    Failure::Refused("gh api graphql: unreadable branches".into())
}

fn parsed(reply: &str) -> Result<Value, Failure> {
    serde_json::from_str(reply).map_err(|_| unreadable())
}

/// The branch name, or a commit headline or Item:/Closes/Fixes/Refs line, names the issue; a
/// passing mention in a commit body does not.
pub(crate) fn names_issue(branch: &str, messages: &[String], repo: &str, number: u64) -> bool {
    branch_names(branch, number) || messages.iter().any(|message| commit_refs(message, repo, number))
}

fn branch_names(branch: &str, number: u64) -> bool {
    Regex::new(&format!(r"(?:^|[-_/#]){number}(?:$|[-_/])")).expect("branch number pattern").is_match(branch)
}

fn commit_refs(message: &str, repo: &str, number: u64) -> bool {
    let lines = python_lines(message);
    lines.iter().enumerate().filter(|(index, line)| *index == 0 || COMMIT_REF.is_match(line.trim())).any(|(_, line)| mentions(line, repo, number))
}

/// `#N` not followed by a digit, after neither a word character, `/` nor `-` unless the repository
/// name is right before it.
fn mentions(text: &str, repo: &str, number: u64) -> bool {
    let wanted = format!("#{number}");
    text.match_indices(&wanted).any(|(at, _)| {
        let after = text[at + wanted.len()..].chars().next();
        let before = text[..at].chars().next_back();
        !after.is_some_and(|c| c.is_numeric()) && (before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '/' || c == '-')) || text[..at].ends_with(repo))
    })
}

/// Lines split the way Python's `str.splitlines` splits them.
pub(crate) fn python_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(end) = rest.find(['\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}', '\u{2029}']) else {
            lines.push(rest);
            break;
        };
        lines.push(&rest[..end]);
        let width = if rest[end..].starts_with("\r\n") { 2 } else { rest[end..].chars().next().map_or(1, char::len_utf8) };
        rest = &rest[end + width..];
    }
    lines
}
