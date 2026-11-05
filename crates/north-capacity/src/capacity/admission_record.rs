//! Admission's lines in the decision log: the host part of the facts as a snapshot written once per
//! distinct reading, and each call as its request part, the snapshot's hash and the reason given.
use north_strategy_types::Kind;
use north_strategy_types::admission::{Case, Decision, Request, Snapshot};
use serde_json::Value;

use super::admission::{Admission, Facts};
use super::native_hold::PairLease;
use crate::platform::decision_log::{self, Entry};

/// The facts as gathered before admission derives its holds, with the pair leases the queue directory holds.
pub struct Gathered {
    pub snapshot: Snapshot,
    pub request: Request,
}

pub fn gathered(facts: &Facts, queued_pairs: &[PairLease]) -> Option<Gathered> {
    let Value::Object(mut fields) = serde_json::to_value(facts).ok()? else { return None };
    fields.insert("queuedPairLeases".to_owned(), serde_json::to_value(queued_pairs).ok()?);
    fields.insert("withinExclusive".to_owned(), Value::Bool(facts.within_exclusive));
    let fields = Value::Object(fields);
    Some(Gathered { snapshot: serde_json::from_value(fields.clone()).ok()?, request: serde_json::from_value(fields).ok()? })
}

/// Logs one call on `gathered` with the disk reading `disk_free_gib` it was decided on, and returns its id.
pub fn record(gathered: &mut Gathered, disk_free_gib: Option<f64>, admitted: &Admission, reclaim: bool, follows: Option<&str>, decide_us: u64) -> String {
    gathered.snapshot.disk_free_gib = disk_free_gib;
    let at = gathered.request.now / 1000.0;
    let at = if at > 0.0 { at } else { decision_log::now() };
    let case = Case { request: gathered.request.clone(), snapshot: decision_log::snapshot(at, &gathered.snapshot), settle_seconds: super::pressure_target::settle_seconds() };
    let decision = Decision {
        reason: serde_json::to_value(admitted.reason).ok().and_then(|reason| serde_json::from_value(reason).ok()).unwrap_or(north_strategy_types::admission::Reason::Invalid),
        offload: admitted.offload.clone(),
        bar1_need_mib: admitted.aperture.map(|aperture| aperture.bar1_need_mib),
        disk_free_gib: admitted.disk_free_gib,
        pair: admitted.pair.as_ref().map(|pair| north_strategy_types::admission::PairRefusal { pair: pair.pair, holder: pair.holder.clone(), free_pair: pair.free_pair }),
        quiet_owners: admitted.quiet_owners.clone(),
        reclaim,
    };
    decision_log::record(Entry { kind: Kind::Admission, at, follows, case: &case, decision: &decision, decide_us })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fact_admission_reads_lands_in_the_snapshot_or_the_request() {
        let facts = super::super::fixtures::facts(super::super::admission::Class::Heavy);
        let gathered = gathered(&facts, &[]).expect("facts split into a snapshot and a request");
        let Value::Object(all) = serde_json::to_value(&facts).unwrap() else { panic!() };
        let Value::Object(snapshot) = serde_json::to_value(&gathered.snapshot).unwrap() else { panic!() };
        let Value::Object(request) = serde_json::to_value(&gathered.request).unwrap() else { panic!() };
        let derived = ["quietNative", "pairLeases", "aloneQueuedAhead"];
        let missing: Vec<&String> = all.keys().filter(|key| !derived.contains(&key.as_str()) && !snapshot.contains_key(*key) && !request.contains_key(*key)).collect();
        assert!(missing.is_empty(), "facts recorded nowhere: {missing:?}");
    }
}
