//! What a dispatch run reads to revive stranded sessions: every host's `north fleet stranded
//! --json`, the saturation refusal of each machine an unlanded strand lives on, and the report each
//! session was last revived for.
use super::fleets;
use super::revive::Strand;
use crate::fleet::stranded::Stranded;
use crate::ops::pacing::placement;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use toml_edit::DocumentMut;

const PROBE: Duration = Duration::from_secs(120);

pub(crate) struct Read {
    pub strands: Vec<Strand>,
    pub saturated: BTreeMap<String, String>,
    pub revived: BTreeMap<String, u64>,
}

pub(crate) fn read(cfg: &DocumentMut, here: &str, at: f64, notes: &mut Vec<String>) -> Read {
    let listings = fleets::everywhere(cfg, here, &["fleet", "stranded", "--json"], "stranded", at, PROBE, notes);
    let strands = strands(&listings, notes);
    let saturated = saturated(&strands);
    let revived = serde_json::from_value(placement::pace_state(&format!("dispatch/{}", super::effects::REVIVED))).unwrap_or_default();
    Read { strands, saturated, revived }
}

/// The sessions each host's listing names, with the host whose registry holds them.
fn strands(listings: &[(String, Value)], notes: &mut Vec<String>) -> Vec<Strand> {
    let mut found = Vec::new();
    for (registry, listing) in listings.iter().filter(|(_, listing)| !listing.is_null()) {
        match serde_json::from_value::<Vec<Stranded>>(listing.clone()) {
            Ok(sessions) => found.extend(sessions.into_iter().map(|session| Strand { session, registry: registry.clone() })),
            Err(why) => notes.push(format!("{registry} fleet stranded unparsed, so its stranded lanes wait: {why}")),
        }
    }
    found
}

fn saturated(strands: &[Strand]) -> BTreeMap<String, String> {
    let hosts: BTreeSet<&str> = strands.iter().filter(|strand| !strand.session.landed()).map(Strand::host).collect();
    hosts.into_iter().filter_map(|host| placement::saturation_refusal(host).unwrap_or_else(Some).map(|why| (host.to_owned(), why))).collect()
}
