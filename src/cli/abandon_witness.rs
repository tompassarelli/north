use std::collections::BTreeSet;

use crate::fleet::landings;
use crate::land_abandon::{ScratchPush, Witness};

pub(super) struct FleetWitness;

impl Witness for FleetWitness {
    fn live_sessions(&self) -> Option<BTreeSet<String>> {
        super::fleet::open_fleet().ok()?.live_owners()
    }

    fn pushes(&self) -> Vec<ScratchPush> {
        landings::pushes(&landings::push_log())
            .into_iter()
            .map(|push| ScratchPush { at_s: push.at_ms / 1_000, repository: push.repository, branch: push.branch, oid: push.oid, session: push.session })
            .collect()
    }
}
