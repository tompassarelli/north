//! Admission calls replayed on the snapshots they were decided on.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use north_base::platform::decision_log;
use north_strategy_types::admission::{Case, Decision, Reason};
use north_strategy_types::{Envelope, Kind};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::report::{Counts, Invariant, stamp};
use crate::{Deciders, parallel};

type Hash = [u8; 24];

#[derive(Deserialize)]
struct Stored<'a> {
    hash: &'a str,
    #[serde(borrow)]
    snapshot: &'a RawValue,
}

const SLOT_RULE: &str = "a lease asked from inside an offload slot is never deferred behind the land check waiting for that slot";

fn key(text: &str) -> Option<Hash> {
    text.as_bytes().try_into().ok()
}

fn index_of(files: &[PathBuf]) -> HashMap<Hash, usize> {
    let mut index = HashMap::new();
    for (file, plain) in files.iter().enumerate() {
        for line in decision_log::read_hour(plain).lines() {
            let Ok(stored) = serde_json::from_str::<Stored>(line) else { continue };
            let Some(key) = key(stored.hash) else { continue };
            index.insert(key, file);
        }
    }
    index
}

fn held_for(files: &[PathBuf], index: &HashMap<Hash, usize>, cases: &[Envelope<Case, Decision>]) -> HashMap<Hash, Box<RawValue>> {
    let mut wanted: BTreeMap<usize, HashSet<Hash>> = BTreeMap::new();
    for logged in cases {
        let Some(key) = key(&logged.case.snapshot) else { continue };
        let Some(&file) = index.get(&key) else { continue };
        wanted.entry(file).or_default().insert(key);
    }
    let mut held = HashMap::new();
    for (file, hashes) in wanted {
        for line in decision_log::read_hour(&files[file]).lines() {
            let Ok(stored) = serde_json::from_str::<Stored>(line) else { continue };
            let Some(key) = key(stored.hash) else { continue };
            if hashes.contains(&key) && index.get(&key) == Some(&file) {
                held.insert(key, stored.snapshot.to_owned());
            }
        }
    }
    held
}

fn label(reason: Reason) -> String {
    serde_json::to_value(reason).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default()
}

/// Replays every admission call in the window; the invariant is that a request asked from inside an
/// offload slot is never deferred behind the land check that waits for that slot.
pub fn replay(root: &Path, since: f64, now: f64, deciders: &Deciders) -> (Counts, Invariant) {
    let snapshot_files = decision_log::hour_files(root, Kind::Admission, "snapshots.jsonl", since - 3600.0, now);
    let index = index_of(&snapshot_files);
    let mut counts = Counts::new("admission");
    let mut slot = Invariant::new("slot-time", SLOT_RULE.to_owned());
    for lines in decision_log::hourly_lines(root, Kind::Admission, "jsonl", since, now) {
        let cases: Vec<Envelope<Case, Decision>> = lines.iter().filter_map(|(_, line)| serde_json::from_str(line).ok()).filter(|logged: &Envelope<Case, Decision>| logged.at >= since).collect();
        let held = held_for(&snapshot_files, &index, &cases);
        let parts = parallel::chunks(&cases, |part| {
            let mut counts = Counts::new("admission");
            let mut slot = Invariant::new("slot-time", SLOT_RULE.to_owned());
            for logged in part {
                let replayed = key(&logged.case.snapshot).and_then(|key| held.get(&key)).and_then(|snapshot| counts.timed(deciders.clock, || (deciders.admission)(snapshot.get(), &logged.case.request)));
                counts.record(&label(logged.decision.reason), replayed.map(|(reason, _)| label(reason)).as_deref());
                let Some((reason, claimed)) = replayed else { continue };
                counts.compare(logged.at, logged.case.request.ticket.as_deref().unwrap_or("-"), &label(logged.decision.reason), &label(reason));
                if logged.case.request.holds_offload_slot && claimed {
                    slot.check(reason != Reason::DeferLandCheckWaiting, || {
                        format!("{}: the {:?} lease {:?} asked from inside an offload slot is deferred behind the waiting land check, so the slot stays held", stamp(logged.at), logged.case.request.class, logged.case.request.ticket)
                    });
                }
            }
            (counts, slot)
        });
        for (part_counts, part_slot) in parts {
            counts.merge(part_counts);
            slot.merge(part_slot);
        }
    }
    (counts, slot)
}
