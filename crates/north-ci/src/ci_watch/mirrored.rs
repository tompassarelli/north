use north_github_mirror::store::{self, Mirror};
use serde_json::Value;

use super::TITLE;

pub(super) const ALL_COMMENTS_BELOW: usize = 10;

fn fresh(mirror: &Mirror, at: f64, repo: &str, kind: &str) -> Option<Value> {
    mirror.fresh(&store::key(store::mirrored(repo)?, kind), at)
}

pub(super) fn red_issues(mirror: &Mirror, at: f64, repo: &str) -> Option<Vec<Value>> {
    let nodes = fresh(mirror, at, repo, store::OPEN_ISSUES)?;
    Some(nodes.as_array()?.iter().filter(|issue| issue["title"] == TITLE).cloned().collect())
}

pub(super) fn complete_comments(issue: &Value) -> Option<Vec<String>> {
    let comments = issue["comments"]["nodes"].as_array().filter(|comments| comments.len() < ALL_COMMENTS_BELOW)?;
    Some(comments.iter().filter_map(|comment| comment["body"].as_str().map(str::to_owned)).collect())
}

pub(super) fn main_runs(mirror: &Mirror, at: f64, repo: &str) -> Option<Vec<Value>> {
    fresh(mirror, at, repo, store::MAIN_RUNS)?["workflow_runs"].as_array().cloned()
}

pub(super) fn url(repo: &str, number: u64) -> String {
    format!("https://github.com/{repo}/issues/{number}")
}

#[cfg(test)]
#[path = "mirrored_tests.rs"]
mod tests;
