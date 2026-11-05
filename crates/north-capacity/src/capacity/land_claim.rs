//! A land check that finds no check host with room claims each host it may run on; while the claim stands, the host
//! admits no new worker build or test lease, so the cores running ones free go to the land check, which unblocks every
//! worker queued behind it. Native, GPU, critical (the land check itself), Tom's and runtime classes keep their rules.
use super::admission::{Class, default_mib};

/// The file under the capacity state root whose modification time is the newest claim.
pub const FILE: &str = "land-check-waiting";
pub const REASON: &str = "land check waiting";

/// Whether a lease of `class` yields to a waiting land check.
pub fn yields(class: Class) -> bool {
    matches!(class, Class::Heavy | Class::Moderate | Class::Exclusive)
}

/// Whether a claim made at `claimed_ms` still stands at `now_ms`; the lander renews it on each placement it fails.
pub fn stands(claimed_ms: f64, now_ms: f64) -> bool {
    now_ms - claimed_ms < default_mib("land_check_claim_seconds") * 1000.0
}
