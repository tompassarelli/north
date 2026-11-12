use std::time::{Duration, Instant};

use super::super::scratch;
use super::super::transcript::{Transcript, shorten};

use super::{HAND_IN_DEADLINE, HAND_IN_TOOL, Journey, short};

pub(super) fn hand_in(journey: &mut Journey) -> Result<String, String> {
    let deadline = journey.clock.now() + HAND_IN_DEADLINE;
    checked(journey, deadline).map_err(|problem| format!("the hand-in step stopped inside its {} s deadline, nine times its slowest passing run (3 to 12 s, north#1273): {problem}", HAND_IN_DEADLINE.as_secs()))
}

fn checked(journey: &Journey, deadline: Instant) -> Result<String, String> {
    journey.settled_report(HAND_IN_DEADLINE, |_| true)?;
    let transcript = journey.north.transcript_within(&journey.session, left(journey, deadline)?)?;
    let scratch = journey.scratch()?;
    let landed = scratch.origin_main(left(journey, deadline)?)?;
    returned(&transcript, &landed)?;
    if landed == scratch.base {
        return Err(format!("the {HAND_IN_TOOL} call returned before origin main moved, so it did not wait for its landing"));
    }
    let head = scratch.lane_head(left(journey, deadline)?)?;
    Ok(format!("{} handed in {} at {}", journey.session, scratch::BRANCH, short(&head)))
}

fn left(journey: &Journey, deadline: Instant) -> Result<Duration, String> {
    let left = deadline.saturating_duration_since(journey.clock.now());
    if left.is_zero() {
        return Err("its time ran out before its checks of the hand-in".to_owned());
    }
    Ok(left)
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

#[cfg(test)]
mod tests;
