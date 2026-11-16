//! The review-accept and safe-push-keys scans read together, as their land check lines run them, so the hand-in and the lander tell a lane waiting only for a reviewer's accept from one they refuse (north#1632).

use std::path::Path;

use north_base::platform::finish_record::FinishRecords;
use north_base::platform::records_mirror::{Records, state_home};
use north_base::platform::review_accept::Accepts;

use crate::{review_accept, safe_push_keys};

const MISSING: &str = "holds no review accept of its patch";

/// What the review-accept and safe-push-keys scans, run as their land check lines run them, make of `base..head`: it passes, it waits only for a reviewer's accept, or they refuse it for anything else.
#[derive(Debug, PartialEq, Eq)]
pub enum Review {
    Passed,
    Awaiting(String),
    Refused(String),
}

pub fn review(directory: &Path, base: &str, head: &str) -> Review {
    let (state, records) = (state_home(), Records::here());
    let (accepts, finished) = (Accepts::counted(&records), FinishRecords::here());
    let range = format!("{base}..{head}");
    let mut out = Vec::new();
    let refused = review_accept::scan(directory, &range, &state, &accepts, &finished, &mut out) != 0;
    let refused = safe_push_keys::scan(directory, &range, &state, &accepts, &finished, &mut out) != 0 || refused;
    let text = String::from_utf8_lossy(&out).trim().to_owned();
    let refusals: Vec<&str> = text.lines().filter(|line| line.contains(": refused: ")).collect();
    match (refused, !refusals.is_empty() && refusals.iter().all(|line| line.contains(MISSING))) {
        (false, _) => Review::Passed,
        (true, true) => Review::Awaiting(refusals.join("\n")),
        (true, false) => Review::Refused(text),
    }
}
