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

const AUTOMATIC_SETTERS: [&str; 3] = ["window", "laptop tick", "laptop-auto"];

pub struct HandWord {
    pub state: String,
    pub since: String,
    pub reason: String,
    pub by: String,
    pub until: Option<String>,
}

/// The drain or close word in a `north fleet laptop status --json` line when a person or session set it; window edges and laptop-auto observations are not set by hand.
pub fn hand_word(status: &str) -> Option<HandWord> {
    let state: State = serde_json::from_str(status.trim()).ok()?;
    (state.state != "available" && !AUTOMATIC_SETTERS.contains(&state.by.as_str())).then_some(HandWord { state: state.state, since: state.since, reason: state.reason, by: state.by, until: state.until })
}
