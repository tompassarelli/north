#![cfg(test)]

use std::collections::BTreeSet;

use super::agent_load::{Lease, Observed, plan};

fn heavy(id: &str, owner: &str) -> Lease {
    Lease { id: id.to_owned(), kind: "run".to_owned(), class: "heavy".to_owned(), aggregate_slice: None, wrapper_pid: None, wrapper_start: Some("1".to_owned()), owner: owner.to_owned() }
}

#[test]
fn on_battery_the_landers_own_checks_keep_running_while_other_heavy_work_freezes_north_1650() {
    let leases = [heavy("a", "land-check"), heavy("b", "gate:worker"), heavy("c", "land-offload")];
    let frozen = BTreeSet::from([leases[2].unit()]);
    let observed = Observed { gaming: false, pause_heavy: true, processes: &[], leases: &leases, frozen: &frozen };
    let planned = plan(&observed, |_| true, |_| true);
    assert_eq!(planned.freeze, BTreeSet::from([leases[1].unit()]), "on 2026-11-18 battery shedding froze the lander's after-landing checks, so three runs passed their deadline");
    assert_eq!(planned.thaw, BTreeSet::from([leases[2].unit()]));
    assert!(planned.stop.is_empty());
}
