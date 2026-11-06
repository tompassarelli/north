use serde_json::Value;

const PAGE: usize = 100;

use crate::store::{CLOSED_ISSUES, Mirror, OPEN_ISSUES, REPOS, key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Open,
    Closed,
}

pub fn now() -> f64 {
    crate::foundation::clock::Timestamp::now().unix_seconds_f64()
}

pub fn listed(mirror: &Mirror, at: f64, repo: &str, kind: &str, miss: impl FnOnce() -> Result<Vec<Value>, String>) -> Result<Vec<Value>, String> {
    let hit = REPOS.contains(&repo).then(|| mirror.fresh(&key(repo, kind), at)).flatten();
    match hit {
        Some(Value::Array(rows)) => Ok(rows),
        _ => miss(),
    }
}

pub fn pages(mirror: &Mirror, at: f64, repo: &str, kind: &str, miss: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    let hit = REPOS.contains(&repo).then(|| mirror.fresh(&key(repo, kind), at)).flatten().filter(whole_page);
    hit.map_or_else(miss, |body| Ok(Value::Array(vec![body])))
}

fn whole_page(body: &Value) -> bool {
    match body {
        Value::Array(rows) => rows.len() < PAGE,
        Value::Object(_) => body["total_count"].as_u64().is_some_and(|total| body["workflow_runs"].as_array().is_some_and(|runs| runs.len() as u64 == total)),
        _ => false,
    }
}

fn classify(open: &Value, closed: &Value, number: u64) -> Option<Seen> {
    let numbered = |rows: &Value| rows.as_array().is_some_and(|rows| rows.iter().any(|row| row["number"] == number || *row == number));
    if numbered(open) {
        return Some(Seen::Open);
    }
    numbered(&closed["numbers"]).then_some(Seen::Closed)
}

pub fn issue_state(mirror: &Mirror, at: f64, repo: &str, number: u64) -> Option<Seen> {
    let hit = |kind: &str| REPOS.contains(&repo).then(|| mirror.fresh(&key(repo, kind), at)).flatten();
    classify(&hit(OPEN_ISSUES)?, &hit(CLOSED_ISSUES)?, number)
}

pub fn recorded_issue_state(mirror: &Mirror, repo: &str, number: u64) -> Option<Seen> {
    let copy = |kind: &str| REPOS.contains(&repo).then(|| mirror.read(&key(repo, kind))).flatten().map(|copy| copy.body);
    classify(&copy(OPEN_ISSUES)?, &copy(CLOSED_ISSUES)?, number)
}

pub fn pages_site(mirror: &Mirror, at: f64, repo: &str, miss: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    let hit = REPOS.contains(&repo).then(|| mirror.fresh(&key(repo, crate::audit::PAGES), at)).flatten();
    hit.map_or_else(miss, Ok)
}

pub fn repo_list(mirror: &Mirror, at: f64, miss: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    mirror.fresh(crate::audit::ACCOUNT_REPOS, at).filter(Value::is_array).map_or_else(miss, Ok)
}

pub fn compared(mirror: &Mirror, repo: &str, branch: &str, base: &str, head: &str) -> Option<Value> {
    let copy = REPOS.contains(&repo).then(|| mirror.read(&key(repo, &crate::audit::compare_kind(branch)))).flatten()?.body;
    let same_tip = copy["head"] == head;
    (same_tip && (copy["base"] == base || copy["ahead_by"] == 0)).then_some(copy)
}

pub fn compare(mirror: &Mirror, repo: &str, branch: &str, base: &str, head: &str, miss: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    compared(mirror, repo, branch, base, head).map_or_else(miss, Ok)
}
