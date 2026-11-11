use super::super::scratch;
use super::super::transcript::shorten;

use super::{HAND_IN_TOOL, HAND_IN_WAIT, Journey, short};

pub(super) fn hand_in(journey: &mut Journey) -> Result<String, String> {
    let report = journey.settled_report(HAND_IN_WAIT, |_| true)?;
    let transcript = journey.transcript()?;
    if !transcript.entries.iter().any(|entry| entry.kind == "tool" && entry.text.starts_with(HAND_IN_TOOL)) {
        let calls: Vec<&str> = transcript.entries.iter().filter(|entry| entry.kind == "tool").map(|entry| entry.text.as_str()).collect();
        return Err(format!("the worker did not call the {HAND_IN_TOOL} tool; its tool calls were {:?}", shorten(&calls.join(" | "))));
    }
    if !report.starts_with("Done: landed") {
        return Err(format!("the worker's hand-in did not return `Done: landed`; it reported {:?}", shorten(&report)));
    }
    let scratch = journey.scratch()?;
    let head = scratch.lane_head()?;
    if scratch.origin_main()? == scratch.base {
        return Err(format!("the worker reported {:?} before origin main moved, so it did not wait for its landing", shorten(&report)));
    }
    Ok(format!("{} handed in {} at {}", journey.session, scratch::BRANCH, short(&head)))
}
