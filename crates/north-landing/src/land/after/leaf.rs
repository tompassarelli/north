use std::fmt;
use std::path::Path;

use north_base::platform::finish_record::FinishRecords;

use crate::issue_body::{self, Edit, Issue, Tracker};
use crate::land::candidate::git;

pub const WAITS: &str = "waits:runtime";
const DONE_WHEN: &str = "Done when: `";
const SHORT_SHA: usize = 12;

pub struct Landed<'a> {
    pub repo: &'a Path,
    pub tip: &'a str,
    pub main: &'a str,
    pub check: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Settled {
    Closed,
    Boxes,
    AlreadyClosed,
    OpenChildren(u64),
    Waits(String),
}

impl fmt::Display for Settled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Settled::Closed => write!(formatter, "closed by its Done-when command"),
            Settled::Boxes => write!(formatter, "left open: it has checkboxes, which their own landings tick"),
            Settled::AlreadyClosed => write!(formatter, "already closed"),
            Settled::OpenChildren(count) => write!(formatter, "left open: {count} of its children are open"),
            Settled::Waits(why) => write!(formatter, "left open: {why}"),
        }
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(SHORT_SHA)]
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

pub fn done_when(body: &str) -> Option<&str> {
    let (_, after) = body.split_once(DONE_WHEN)?;
    let (command, _) = after.split_once('`')?;
    Some(command.trim()).filter(|command| !command.is_empty())
}

fn same(left: &str, right: &str) -> bool {
    left.split_whitespace().eq(right.split_whitespace())
}

pub fn passed(records: &FinishRecords, session: &str, repo: &Path, head: &str) -> Option<String> {
    let finished = records.read(session).filter(|finished| finished.passed && !finished.head.is_empty())?;
    let tree = |commit: &str| git(repo, &["rev-parse", "--verify", "--quiet", &format!("{commit}^{{tree}}")]).ok();
    (tree(&finished.head)? == tree(head)?).then_some(finished.command)
}

pub fn queue(queue_directory: &Path, landed: &Landed) -> Vec<String> {
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
    items
        .into_iter()
        .map(|issue| {
            let edit = Edit::Close { sha: landed.tip.to_owned(), check: landed.check.map(str::to_owned) };
            match issue_body::submit(queue_directory, &issue, &edit) {
                Ok(_) => format!("queued the close of {issue} by its check, landed in {}", short(landed.tip)),
                Err(error) => format!("cannot queue the close of {issue}: {error}"),
            }
        })
        .collect()
}

pub fn settle(github: &dyn Tracker, issue: &Issue, sha: &str, check: Option<&str>) -> Result<Settled, String> {
    let body = github.body(issue)?;
    if issue_body::has_checkbox(&body) {
        return Ok(Settled::Boxes);
    }
    let standing = github.standing(issue)?;
    if !standing.open {
        return Ok(Settled::AlreadyClosed);
    }
    if standing.open_children > 0 {
        return Ok(Settled::OpenChildren(standing.open_children));
    }
    let sha = short(sha);
    let why = match (done_when(&body), check) {
        (Some(command), Some(check)) if same(command, check) => {
            github.close(issue, &format!("Closed by the landing of {sha}: its Done-when command `{command}` passed on that commit."))?;
            return Ok(Settled::Closed);
        }
        (Some(command), Some(check)) => format!("the check that passed on {sha} was `{check}`, not its Done-when command `{command}`"),
        (Some(command), None) => format!("its Done-when command `{command}` has not passed on {sha}"),
        (None, _) => "its body names no Done-when command".to_owned(),
    };
    Ok(Settled::Waits(match github.label(issue, WAITS) {
        Ok(()) => format!("{why}; labelled {WAITS}"),
        Err(error) => format!("{why}; not labelled {WAITS}: {error}"),
    }))
}

#[cfg(test)]
#[path = "leaf_tests.rs"]
mod tests;
