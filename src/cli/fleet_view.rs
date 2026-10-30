//! This host's fleet registry as the threads ledger reads it: live owners and the issues open
//! gates decide, opened afresh on each read.
use std::collections::{BTreeMap, BTreeSet};

use super::fleet::open_fleet;
use crate::threads::outside::FleetView;

pub(super) struct Registry;

impl FleetView for Registry {
    fn live_owners(&self) -> Option<BTreeSet<String>> {
        open_fleet().ok()?.live_owners()
    }

    fn gated_issues(&self) -> BTreeMap<String, String> {
        open_fleet().and_then(|fleet| fleet.gated_issues()).unwrap_or_default()
    }
}
