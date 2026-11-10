//! `north fleet break-glass --issue REPO#N REASON...`: lifts the proxy's look budgets for at most 30 minutes while the harness
//! itself measures that workers cannot start, records who, why and when, and opens a box on the issue that tracks the cause.
use crate::error::{NorthError, NorthResult};
use north_guard::guard::break_glass::{self, Live, Refusal, USAGE};

fn parse<'a>(words: &[&'a str]) -> NorthResult<(&'a str, String)> {
    let usage = || NorthError::Usage(USAGE.into());
    let (flag, rest) = words.split_first().ok_or_else(usage)?;
    let (issue, reason) = rest.split_first().filter(|_| *flag == "--issue").ok_or_else(usage)?;
    Ok((issue, reason.join(" ")))
}

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let (issue, reason) = parse(words)?;
    match break_glass::invoke(&Live::here(), &reason, issue) {
        Ok(grant) => {
            println!("break glass: the proxy's look budgets are lifted for {} minutes ({}); a box is open on {issue}", break_glass::minutes(), grant.evidence);
            Ok(())
        }
        Err(refusal @ (Refusal::NoReason | Refusal::WorkersCanLaunch(_) | Refusal::Unrecorded(_))) => Err(NorthError::Refused(refusal.message())),
        Err(refusal) => Err(NorthError::State(refusal.message())),
    }
}
