//! Whether a worker's report has already withheld an issue from staffing: a `Blocked:` comment from
//! the last week or the time-gated label. `threads ready`, `threads unowned` and dispatch share it.
use serde_json::Value as Json;

use crate::foundation::clock::Timestamp;
use crate::threads::staffing::branches::python_lines;

const BLOCK_DAYS: i64 = 7;
const TIME_GATED: &str = "evidence:time-gated";

/// Why a worker's report keeps the issue from being staffed again: a `Blocked:` comment from the
/// last week, else the time-gated label.
pub fn withheld(labels: &[&str], comments: &[Json], now: f64) -> Option<String> {
    comment_block(comments, now).or_else(|| labels.contains(&TIME_GATED).then(|| "time-gated".to_owned()))
}

/// The reason of the newest comment from the last week that starts `Blocked:`, or `comment` when
/// it gives none; an older comment ends the search.
pub fn comment_block(comments: &[Json], now: f64) -> Option<String> {
    let cutoff = now - (BLOCK_DAYS * 86400) as f64;
    let mut newest: Vec<&Json> = comments.iter().collect();
    newest.sort_by(|left, right| right["createdAt"].as_str().cmp(&left["createdAt"].as_str()));
    for comment in newest {
        let created = comment["createdAt"].as_str().and_then(|at| Timestamp::parse(at).ok()).map_or(0.0, Timestamp::unix_seconds_f64);
        if created < cutoff {
            return None;
        }
        if let Some(reason) = comment["body"].as_str().unwrap_or_default().trim().strip_prefix("Blocked:") {
            return Some(python_lines(reason.trim()).first().map_or_else(|| "comment".to_owned(), |line| (*line).to_owned()));
        }
    }
    None
}
