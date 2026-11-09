use std::collections::BTreeSet;

use crate::cone::Cone;
use crate::facts::TestId;

pub struct Pass {
    pub lane: String,
    pub ran: Cone,
    pub unseen: Cone,
}

pub fn carried(lanes: &[(String, Cone)], passes: &[Pass]) -> BTreeSet<TestId> {
    if lanes.iter().any(|(_, cone)| cone.untraced.is_some()) {
        return BTreeSet::new();
    }
    let reach = |cone: &Cone, test: &TestId| cone.tests.contains(test) || cone.always.contains(test);
    let mut carried = BTreeSet::new();
    for pass in passes.iter().filter(|pass| pass.ran.untraced.is_none() && pass.unseen.untraced.is_none()) {
        for test in pass.ran.tests.iter().chain(&pass.ran.always).filter(|test| !reach(&pass.unseen, test)) {
            let mut holders = lanes.iter().filter(|(_, cone)| reach(cone, test)).map(|(name, _)| name.as_str());
            if holders.next() == Some(pass.lane.as_str()) && holders.next().is_none() {
                carried.insert(test.clone());
            }
        }
    }
    carried
}
