//! What the lifecycle machine detectors read: the incident fixes `north check machines --fixes` reports for the
//! checkout, and the moves every host's lifecycle stores refused.
use std::fs;
use std::path::Path;

use crate::detectors::{machine_fix, machine_refusal};
use crate::flow::{Host, SSH, remote_hosts};
use crate::health::flows::facts;
use crate::health::flows::probe::REMOTE_FACTS;
use crate::health::flows::{HostFacts, Lifecycles};

use super::{Gatherer, owner_line};

const MACHINE_DOC: &str = "state-machines";

impl Gatherer<'_> {
    fn machine_owner(&self) -> Option<String> {
        fs::read_to_string(self.checkout().join(format!("docs/rationale/{MACHINE_DOC}.md"))).ok().and_then(|text| owner_line(&text))
    }

    pub fn machine_fixes(&self) -> machine_fix::Facts {
        let report = self.host.run(&["north", "check", "machines", "--fixes", "--root", &self.checkout().to_string_lossy()], None, None).unwrap_or_default();
        machine_fix::Facts { owner: self.machine_owner(), fixed: machine_fix::census(&report) }
    }

    pub fn machine_refusals(&self) -> machine_refusal::Facts {
        let here = crate::platform::host::name();
        let local = (here.clone(), facts::lifecycles(&self.places.home));
        let remote = remote_lifecycles(self.host, &crate::platform::orchestration::path(), &here);
        let faults = std::iter::once(local).chain(remote).flat_map(|(host, lifecycles)| host_faults(host, lifecycles)).collect();
        machine_refusal::Facts { owner: self.machine_owner(), faults }
    }
}

/// Each other host's lifecycle records, read as `north health facts` prints them there over ssh; a host that does not
/// answer, or prints no facts, is left out.
fn remote_lifecycles(host: &dyn Host, policy: &Path, here: &str) -> Vec<(String, Lifecycles)> {
    remote_hosts(policy)
        .into_iter()
        .filter(|(name, _)| name != here)
        .filter_map(|(name, destination)| {
            let mut argv: Vec<&str> = SSH.to_vec();
            argv.extend([destination.as_str(), REMOTE_FACTS]);
            let facts: HostFacts = serde_json::from_str(&host.run(&argv, None, None)?).ok()?;
            Some((name, facts.lifecycles))
        })
        .collect()
}

fn host_faults(host: String, lifecycles: Lifecycles) -> Vec<machine_refusal::HostFault> {
    match lifecycles {
        Lifecycles::Read(faults) => faults.into_iter().map(|fault| machine_refusal::HostFault { host: host.clone(), fault }).collect(),
        Lifecycles::Unreadable(_) => Vec::new(),
    }
}
