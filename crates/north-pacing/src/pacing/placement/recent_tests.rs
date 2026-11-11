use super::super::case;
use north_strategy_types::placement::Case;
use std::collections::BTreeMap;

const BURST_CASE: &str = include_str!("fixtures/burst-2026-11-12.json");

#[test]
fn a_burst_decided_against_one_unmoved_reading_spreads_over_the_free_hosts_north_1191() {
    let recorded: Case = serde_json::from_str(BURST_CASE).unwrap();
    let mut placed: BTreeMap<String, i64> = BTreeMap::new();
    for _ in 0..21 {
        let mut case = recorded.clone();
        for machine in &mut case.machines {
            machine.placed_recently = placed.get(&machine.name).copied().unwrap_or(0);
        }
        let machine = case::decide(&case).unwrap().machine.expect("a build host takes the launch").name;
        *placed.entry(machine).or_insert(0) += 1;
    }
    let on = |host: &str| placed.get(host).copied().unwrap_or(0);
    assert!(on("gex45") <= 1, "the 21 launches of 2026-11-12 read gex45 at load 44.1 on 20 threads with 5 sessions, cpx62 at 8.4 and cpx62b at 21.3 on 16, and all 21 went to gex45: {placed:?}");
    assert!(on("cpx62") > on("cpx62b") && on("cpx62b") >= 5, "the emptier hosts take the burst, the emptiest the most: {placed:?}");
}
