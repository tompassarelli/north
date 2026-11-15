//! The review request north sends the reviewer sessions of harness/defaults.toml's `review_request_sessions` when no review call of a patch runs (north#1610): outside a worker domain, on a host that does not settle the fleet, or when the call gave no answer or its input is over the cap. The request goes once per key, a patch id where the caller knows it, so a refused hand-in of the same patch sends none again.

use std::fs;
use std::process::{Command, Stdio};

use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::north_binary::land_north;
use north_base::platform::records_mirror::state_home;

const SESSIONS: &str = "review_request_sessions";
const SENT: &str = "north/land/review-requests";

/// What a refusal of the head `head` of `lane` because of `why` tells its author: the request for `key` went to the reviewer sessions, or the request for `key` went out before, or why none reached a reviewer.
pub fn request(key: &str, lane: &str, head: &str, why: &str) -> String {
    let text = format!("Review request, sent by north because no review call of head {head} in lane {lane} runs: {why}. Read the lane with `north runtime review-fetch {lane}` on the host that settles the fleet, review it, and accept it with `north runtime accept {head}`.");
    match send(key, &text) {
        Ok((sent, true)) => format!("{why}; the review request went to {}", sent.join(" and ")),
        Ok((sent, false)) => format!("{why}; the review request for {key} already went to {} and is not sent again", sent.join(" and ")),
        Err(error) => format!("{why}; {error}"),
    }
}

/// Sends `text` to the reviewer sessions unless `key` has a record of an earlier send, returning the reviewers reached and whether this send was the first.
fn send(key: &str, text: &str) -> Result<(Vec<String>, bool), String> {
    let record = state_home().join(SENT).join(key);
    if let Ok(earlier) = fs::read_to_string(&record) {
        return Ok((earlier.lines().map(str::to_owned).collect(), false));
    }
    let names = document(DEFAULTS)[SESSIONS]["value"].as_array().into_iter().flatten().filter_map(|name| name.as_str());
    let sent: Vec<String> = names.filter(|reviewer| Command::new(land_north()).args(["fleet", "send", reviewer, text]).stdin(Stdio::null()).stdout(Stdio::null()).status().is_ok_and(|status| status.success())).map(str::to_owned).collect();
    if sent.is_empty() {
        return Err(format!("`north fleet send` reached none of the reviewers in {SESSIONS}"));
    }
    let failure = |error: std::io::Error| format!("the request went to {}, but was not recorded at {}: {error}", sent.join(" and "), record.display());
    record.parent().map_or(Ok(()), fs::create_dir_all).map_err(failure)?;
    fs::write(&record, sent.join("\n")).map_err(failure)?;
    Ok((sent, true))
}
