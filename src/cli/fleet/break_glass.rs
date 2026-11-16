//! `north fleet break-glass [--issue REPO#N] --reason REASON...`: lifts the coordinator's reach guard while north's landing
//! is stalled, or for at most 30 minutes while the harness itself measures an incident (workers cannot start, the laptop load alarm fires, or the coordinator host has just rebooted),
//! records who, why and when, and opens a box on the issue that tracks a launch stall.
use crate::error::{NorthError, NorthResult};
use crate::platform::host;
use north_guard::guard::break_glass::{self, Live, Refusal, USAGE};

fn parse<'a>(words: &[&'a str]) -> NorthResult<(Option<&'a str>, String)> {
    let usage = || NorthError::Usage(USAGE.into());
    let (issue, rest) = match words {
        [flag, value, rest @ ..] if *flag == "--issue" => (Some(*value), rest),
        _ => (None, words),
    };
    let reason = rest.strip_prefix(&["--reason"]).unwrap_or(rest);
    if reason.is_empty() {
        return Err(usage());
    }
    Ok((issue, reason.join(" ")))
}

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let (issue, reason) = parse(words)?;
    match break_glass::invoke(&Live::here().booted_at(host::booted_ms()), &reason, issue) {
        Ok(grant) => {
            match &grant.issue {
                Some(issue) => println!("break glass: the coordinator's reach guard is lifted for {} minutes ({}); a box is open on {issue}", break_glass::minutes(), grant.evidence),
                None => println!("break glass: the coordinator's reach guard is lifted until the landing stall clears ({}); `north ledger flow --stall-alarm` reports when it clears", grant.evidence),
            }
            Ok(())
        }
        Err(refusal @ (Refusal::NoReason | Refusal::NoIncident(_) | Refusal::NoIssue(_) | Refusal::Unrecorded(_))) => Err(NorthError::Refused(refusal.message())),
        Err(refusal) => Err(NorthError::State(refusal.message())),
    }
}
