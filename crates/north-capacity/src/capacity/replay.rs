//! Admission's decision on a logged call, decided again by the code this binary links: the call's
//! facts are rebuilt from its snapshot and its request exactly as `enrich` split them.
use north_strategy_types::admission::{Reason, Request};
use serde::Deserialize;

use super::admission::{self, Facts};
use super::native_hold::PairLease;

#[derive(Deserialize)]
struct Queued {
    #[serde(default, rename = "queuedPairLeases")]
    pairs: Vec<PairLease>,
}

/// The reason admission gives `request` on the logged snapshot object `snapshot`, with whether that snapshot held a land check's claim.
/// The snapshot and request share no field, so their JSON objects join into the facts without a tree between.
pub fn decide(snapshot: &str, request: &Request) -> Option<(Reason, bool)> {
    let queued: Queued = serde_json::from_str(snapshot).ok()?;
    let request_text = serde_json::to_string(request).ok()?;
    let joined = format!("{},{}", snapshot.strip_suffix('}')?, request_text.strip_prefix('{')?);
    let mut facts: Facts = serde_json::from_str(&joined).ok()?;
    facts.within_exclusive = request.within_exclusive;
    facts.holds_offload_slot = request.holds_offload_slot;
    facts.derive_native_holds();
    facts.pair_leases.extend(queued.pairs);
    let reason = serde_json::from_value(serde_json::to_value(admission::decide(&facts)).ok()?).ok()?;
    Some((reason, facts.land_check_waiting))
}
