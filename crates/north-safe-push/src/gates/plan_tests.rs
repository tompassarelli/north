use std::collections::HashSet;

use super::{Destination, Gate, GateFacts, SecretScannerMissing, plan};

const SCANS: [&str; 10] = [
    "dependency gate",
    "native item reference",
    "tool removal",
    "test integrity",
    "test admission",
    "policy sweep",
    "document owners",
    "secret scan",
    "deny-list scan",
    "commit email",
];

fn labels(destination: Destination, land_checks_present: bool) -> Vec<&'static str> {
    let facts = GateFacts { destination, land_checks_present, betterleaks_on_path: true };
    plan(&facts).expect("betterleaks is on PATH").into_iter().map(Gate::label).collect()
}

fn expected(lander_gates: &[&'static str], shell_hook: bool) -> Vec<&'static str> {
    let mut all = vec!["language gate"];
    all.extend_from_slice(lander_gates);
    all.extend(SCANS);
    if shell_hook {
        all.push("shell hook");
    }
    all
}

#[test]
fn the_plan_orders_gates_as_the_refusal_line_names_them() {
    assert_eq!(
        labels(Destination::Default, true),
        expected(&["stale revert of newer main", "land checks", "lane commits dropped by the rebase", "stale revert of newer main"], false)
    );
    assert_eq!(
        labels(Destination::Default, false),
        expected(&["land checks", "lane commits dropped by the rebase", "stale revert of newer main"], false)
    );
    for land_checks_present in [false, true] {
        assert_eq!(labels(Destination::Other, land_checks_present), expected(&[], true));
    }
    a_missing_secret_scanner_refuses_every_plan_and_nothing_else_does();
}

fn a_missing_secret_scanner_refuses_every_plan_and_nothing_else_does() {
    for destination in [Destination::Default, Destination::Other] {
        for land_checks_present in [false, true] {
            let facts = GateFacts { destination, land_checks_present, betterleaks_on_path: false };
            assert_eq!(plan(&facts), Err(SecretScannerMissing), "{facts:?}");
            let planned = plan(&GateFacts { betterleaks_on_path: true, ..facts }).expect("planned");
            assert!(planned.contains(&Gate::Secrets), "{facts:?}");
            assert_eq!(planned[0], Gate::Language, "{facts:?}");
            let distinct = planned.iter().collect::<HashSet<_>>().len();
            assert_eq!(planned.len() - distinct, usize::from(destination == Destination::Default && land_checks_present), "{facts:?}");
        }
    }
}
