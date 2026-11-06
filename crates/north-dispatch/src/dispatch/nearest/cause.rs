//! Whose doing a finished run's outcome was, in the replay's terms: kept, the worker's (missed the box,
//! reworked, ran out of approach or budget) or the harness's. The harness classes and Tom's halts and
//! decisions come from the shared attribution (`north_threads::threads::attribution`, north#706); Tom's
//! count as the harness's here, since neither speaks to a tier's fitness.
use north_threads::threads::attribution;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Cause {
    Kept,
    Worker(&'static str),
    Harness(&'static str),
}

/// The cause of a run's outcome, or None when it says nothing about the worker or the harness: a split,
/// which re-scopes the box, or an unclear end with no reason a rule can read.
pub fn cause(outcome: &str, succeeded: bool, attributed: attribution::Cause) -> Option<Cause> {
    if outcome == "split" {
        return None;
    }
    if succeeded {
        return Some(Cause::Kept);
    }
    match (attributed, outcome) {
        (attribution::Cause::Harness(harness), _) => Some(Cause::Harness(harness.label())),
        (attribution::Cause::Tom, _) => Some(Cause::Harness("decision")),
        (attribution::Cause::Worker, "done") => Some(Cause::Worker("reworked")),
        (attribution::Cause::Worker, "unclear" | "handoff") => None,
        (attribution::Cause::Worker, _) => Some(Cause::Worker("missed")),
    }
}
