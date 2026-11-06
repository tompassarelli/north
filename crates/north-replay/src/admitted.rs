//! Admission calls replayed on the snapshots they were decided on.
use std::collections::HashMap;
use std::path::Path;

use north_base::platform::decision_log;
use north_strategy_types::admission::{Case, Decision, Reason};
use north_strategy_types::{Envelope, Kind};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::report::{Counts, Invariant, stamp};
use crate::{Deciders, parallel};

#[derive(Deserialize)]
struct Held {
    hash: String,
    snapshot: Box<RawValue>,
}

const SLOT_RULE: &str = "a lease asked from inside an offload slot is never deferred behind the land check waiting for that slot";

fn snapshots(root: &Path, since: f64, now: f64) -> HashMap<String, Box<RawValue>> {
    let lines = decision_log::lines(root, Kind::Admission, "snapshots.jsonl", since - 3600.0, now);
    parallel::chunks(&lines, |part| part.iter().filter_map(|(_, line)| serde_json::from_str::<Held>(line).ok()).map(|held| (held.hash, held.snapshot)).collect::<Vec<_>>()).into_iter().flatten().collect()
}

fn label(reason: Reason) -> String {
    serde_json::to_value(reason).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

/// Replays every admission call in the window; the invariant is that a request asked from inside an
/// offload slot is never deferred behind the land check that waits for that slot.
pub fn replay(root: &Path, since: f64, now: f64, deciders: &Deciders) -> (Counts, Invariant) {
    let held = snapshots(root, since, now);
    let lines = decision_log::lines(root, Kind::Admission, "jsonl", since, now);
    let parts = parallel::chunks(&lines, |part| {
        let mut counts = Counts::new("admission");
        let mut slot = Invariant::new("slot-time", SLOT_RULE.to_owned());
        for (_, line) in part {
            let Ok(logged) = serde_json::from_str::<Envelope<Case, Decision>>(line) else { continue };
            if logged.at < since {
                continue;
            }
            let replayed = held.get(&logged.case.snapshot).and_then(|snapshot| (deciders.admission)(snapshot.get(), &logged.case.request));
            counts.record(&label(logged.decision.reason), replayed.map(|(reason, _)| label(reason)).as_deref());
            let Some((reason, claimed)) = replayed else { continue };
            if logged.case.request.holds_offload_slot && claimed {
                slot.check(reason != Reason::DeferLandCheckWaiting, || {
                    format!("{}: the {:?} lease {:?} asked from inside an offload slot is deferred behind the waiting land check, so the slot stays held", stamp(logged.at), logged.case.request.class, logged.case.request.ticket)
                });
            }
        }
        (counts, slot)
    });
    let mut counts = Counts::new("admission");
    let mut slot = Invariant::new("slot-time", SLOT_RULE.to_owned());
    for (part_counts, part_slot) in parts {
        counts.merge(part_counts);
        slot.merge(part_slot);
    }
    (counts, slot)
}
