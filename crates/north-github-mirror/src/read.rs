use serde_json::Value;

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
