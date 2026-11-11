use super::super::scratch;
use super::super::transcript::{Transcript, shorten};

use super::{HAND_IN_TOOL, HAND_IN_WAIT, Journey, short};

pub(super) fn hand_in(journey: &mut Journey) -> Result<String, String> {
    journey.settled_report(HAND_IN_WAIT, |_| true)?;
    let transcript = journey.transcript()?;
    let scratch = journey.scratch()?;
    let landed = scratch.origin_main()?;
    returned(&transcript, &landed)?;
    if landed == scratch.base {
        return Err(format!("the {HAND_IN_TOOL} call returned before origin main moved, so it did not wait for its landing"));
    }
    let head = scratch.lane_head()?;
    Ok(format!("{} handed in {} at {}", journey.session, scratch::BRANCH, short(&head)))
}

fn returned(transcript: &Transcript, landed: &str) -> Result<(), String> {
    let Some(call) = transcript.entries.iter().position(|entry| entry.kind == "tool" && entry.text.starts_with(HAND_IN_TOOL)) else {
        let calls: Vec<&str> = transcript.entries.iter().filter(|entry| entry.kind == "tool").map(|entry| entry.text.as_str()).collect();
        return Err(format!("the worker did not call the {HAND_IN_TOOL} tool; its tool calls were {:?}", shorten(&calls.join(" | "))));
    };
    let verdict = format!("Done: landed {landed} on main");
    match transcript.entries[call + 1..].iter().find(|entry| entry.kind == "result") {
        Some(result) if result.text.lines().last() == Some(verdict.as_str()) => Ok(()),
        Some(result) => Err(format!("the {HAND_IN_TOOL} call returned {:?}, which does not end in `{verdict}`", result.text.lines().last().unwrap_or_default())),
        None => Err(format!("the session records no result of the {HAND_IN_TOOL} call")),
    }
}
