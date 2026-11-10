//! What the lifecycle machine detectors read: the incident fixes `north check machines --fixes` reports for the
//! checkout, and the moves this host's lifecycle store refused.
use std::fs;

use north_base::lifecycle::Store;

use super::{Gatherer, owner_line};
use crate::detectors::{machine_fix, machine_refusal};

const MACHINE_DOC: &str = "state-machines";
const MACHINE_LEDGER: &str = "north/ledger.db";

impl Gatherer<'_> {
    fn machine_owner(&self) -> Option<String> {
        fs::read_to_string(self.checkout().join(format!("docs/rationale/{MACHINE_DOC}.md"))).ok().and_then(|text| owner_line(&text))
    }

    pub fn machine_fixes(&self) -> machine_fix::Facts {
        let report = self.host.run(&["north", "check", "machines", "--fixes", "--root", &self.checkout().to_string_lossy()], None, None).unwrap_or_default();
        machine_fix::Facts { owner: self.machine_owner(), fixed: machine_fix::census(&report) }
    }

    pub fn machine_refusals(&self) -> machine_refusal::Facts {
        let ledger = &self.places.state.join(MACHINE_LEDGER);
        let faults = if ledger.exists() { Store::open(ledger).and_then(|store| store.faults(crate::foundation::clock::Timestamp::now())).unwrap_or_default() } else { Vec::new() };
        machine_refusal::Facts { owner: self.machine_owner(), faults }
    }
}
