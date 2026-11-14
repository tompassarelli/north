use super::inputs::Reads;
use super::*;

/// The fleet's sessions, and the holds they, the claims and the records place on issues and boxes.
pub(super) struct Fleet {
    pub sessions: Vec<(Session, String)>,
    pub elsewhere: BTreeMap<String, i64>,
    pub live: BTreeSet<String>,
    pub listed: BTreeSet<String>,
    pub records: Vec<record::Record>,
    pub holds: BTreeMap<IssueRef, String>,
    pub predicted: BTreeMap<String, Vec<String>>,
    pub box_holds: record::BoxHolds,
    pub box_tries: record::BoxHolds,
}

pub(super) fn sessions(reads: &Reads) -> Fleet {
    let sessions: Vec<(Session, String)> = reads.fleet_lists.iter().flat_map(|(host, listing)| fleets::running_sessions(listing).into_iter().map(|session| (session, host.clone()))).collect();
    let (elsewhere, mut holds) = fleets::tally(&sessions);
    let live: BTreeSet<String> = sessions.iter().map(|(session, _)| session.name.clone()).collect();
    let records = record::read(&record::directory());
    let stopped_sessions = fleets::stopped_names(&reads.fleet_lists);
    holds.extend(reads.claim_lists.iter().flat_map(|(host, listing)| claim_rows(listing, host, &stopped_sessions)));
    let predicted = fleets::predicted_writes(&sessions);
    let mut box_holds = fleets::box_tally(&sessions);
    for (item, slug, why) in reads.claim_lists.iter().flat_map(|(host, listing)| claim_box_rows(listing, host, &stopped_sessions)) {
        box_holds.entry(item).or_default().entry(slug).or_insert(why);
    }
    let listed = fleets::listed(&reads.fleet_lists);
    Fleet { sessions, elsewhere, live, listed, records, holds, predicted, box_holds, box_tries: record::BoxHolds::new() }
}

pub(super) fn place_holds(reads: &Reads, fleet: &mut Fleet, unspent: &BTreeMap<String, String>, gone: &BTreeSet<String>, at: f64) {
    fleet.holds.extend(record::holds(&fleet.records, at, unspent, gone));
    for (item, boxes) in record::box_retries(&fleet.records, at, unspent) {
        let Some(issue) = reads.issues.iter().find(|issue| issue.reference() == item) else { continue };
        let unticked: BTreeSet<String> = issue.takeable(at).filter_map(crate::threads::claims::box_slug).collect();
        for (slug, why) in boxes.into_iter().filter(|(slug, _)| unticked.contains(slug)) {
            fleet.box_tries.entry(item.clone()).or_default().insert(slug, why);
        }
    }
    for (item, boxes) in record::box_launching(&fleet.records, at, unspent, gone) {
        for (slug, why) in boxes {
            fleet.box_holds.entry(item.clone()).or_default().entry(slug).or_insert(why);
        }
    }
    let mut held = gate_holds(reads.gates.iter().flat_map(|(host, listing)| gate_rows(listing, host)), &reads.issues, at, &mut fleet.box_holds);
    held.sort();
    held.dedup();
    fleet.holds.extend(held);
}
