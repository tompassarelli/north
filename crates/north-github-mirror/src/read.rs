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

pub fn issue_state(mirror: &Mirror, at: f64, repo: &str, number: u64) -> Option<Seen> {
    let numbered = |rows: &Value| rows.as_array().is_some_and(|rows| rows.iter().any(|row| row["number"] == number || *row == number));
    let hit = |kind: &str| REPOS.contains(&repo).then(|| mirror.fresh(&key(repo, kind), at)).flatten();
    if numbered(&hit(OPEN_ISSUES)?) {
        return Some(Seen::Open);
    }
    numbered(&hit(CLOSED_ISSUES)?["numbers"]).then_some(Seen::Closed)
}
