//! `north fleet drain on [--until TIME] | off`: Tom's switch that turns the pooled spend reserve off (docs/rationale/spend-rule.md).
use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::rfc3339_seconds;
use crate::pacing::placement;
use crate::spend::drain;

const USAGE: &str = "Usage: north fleet drain on [--until TIME] | off (TIME is RFC 3339 such as 2026-11-10T09:00:00-08:00; run it only on Tom's word to drain)";

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let (on, until) = match words {
        ["off"] => (false, None),
        ["on"] => (true, None),
        ["on", "--until", time] => (true, Some(rfc3339_seconds(time).ok_or_else(|| NorthError::Usage(format!("{USAGE}; cannot read {time}")))?)),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    let agents = placement::pace_directory();
    drain::set(&agents, on, until).map_err(|error| NorthError::Refused(format!("cannot write the drain switch: {error}")))?;
    println!("{}", drain::mode_line(&agents, placement::now(), crate::spend::Rules::configured().reserve));
    Ok(())
}
