use serde::{Deserialize, Serialize};

use super::stamp::parse;

pub(super) const GRACE_CLOSE: i64 = 120;
pub(super) const GRACE_DRAIN: i64 = 900;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct State {
    pub(super) state: String,
    pub(super) since: String,
    pub(super) reason: String,
    pub(super) by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) grace: Option<i64>,
}

pub(super) fn deadline(state: &State) -> Option<i64> {
    let since = parse(&state.since).ok()?;
    match state.state.as_str() {
        "draining" => Some(since + state.grace.unwrap_or(GRACE_DRAIN)),
        "unavailable" => Some(since + state.grace.unwrap_or(0)),
        _ => None,
    }
}
