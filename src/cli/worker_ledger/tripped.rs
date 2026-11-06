use crate::usage::breaker;
use crate::usage::decide::world::Trip;
use crate::watch;

/// The line `--recommend` ends with when the brief's `Item:` is a workstream the spend breaker has tripped.
pub(super) fn line(facets: &str) -> Option<String> {
    let trips = breaker::read_trips(&watch::state_home().join("agents/usage").join(breaker::LEDGER_FILE)).ok()?;
    line_for(facets, &trips)
}

fn line_for(facets: &str, trips: &[Trip]) -> Option<String> {
    let item = facets.split_once("Item:")?.1.split_whitespace().next()?;
    let trip = trips.iter().find(|trip| trip.workstream.0 == item)?;
    Some(format!(
        "tripped: {item} spent ${:.2} against p90 ${:.2}; a launch into it is refused until a newer brief's Budget line states a higher p90 or Tom's answer to a gate names one",
        trip.spent.0, trip.p90.0
    ))
}
