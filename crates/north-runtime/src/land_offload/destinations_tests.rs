use super::*;

const HOSTS: &str = include_str!("../runtime/fixtures/orchestration-hosts.toml");

#[test]
fn only_the_host_that_settles_the_fleet_publishes_the_facts_of_a_host_that_may_not_write_south() {
    let cfg: DocumentMut = HOSTS.parse().expect("fixture");
    assert!(!publishes_host_facts(&cfg, "nexus"));
    assert!(publishes_host_facts(&cfg, "gex45") && publishes_host_facts(&cfg, "whiterabbit"));
    assert_eq!(deploy(&cfg, "whiterabbit").relays_facts, [Relayed { destination: "nexus".into(), host: "nexus".into() }]);
    assert!(deploy(&cfg, "gex45").relays_facts.is_empty());
}

#[test]
fn a_host_sends_builds_only_to_the_hosts_it_reaches_and_one_that_reaches_none_is_polled_instead() {
    let cfg: DocumentMut = HOSTS.parse().expect("fixture");
    assert_eq!(build_hosts(&cfg, "whiterabbit"), [("gex45".to_owned(), "tom@gex45".to_owned())]);
    assert!(build_hosts(&cfg, "gex45").is_empty());
}

#[test]
fn the_laptop_in_any_capacity_mode_competes_for_no_land_check_beside_the_build_hosts_north_1036() {
    use crate::land_offload::room::Standing;
    let cfg: DocumentMut = HOSTS.parse().expect("fixture");
    assert_eq!(crate::land_offload::standing(false, false, Some(&cfg), "whiterabbit"), Standing::Fallback, "on 2026-11-10 at 2:05 PM the away laptop ran 6 land checks at load 40 while cpx62 sat at 8");
    assert_eq!(crate::land_offload::standing(false, false, Some(&cfg), "gex45"), Standing::Peer);
    assert_eq!(crate::land_offload::standing(true, false, Some(&cfg), "gex45"), Standing::Reserved);
    assert_eq!(crate::land_offload::standing(false, true, Some(&cfg), "gex45"), Standing::Reserved, "a laptop on battery takes no land check");
}

#[test]
fn a_host_the_settling_host_ships_to_runs_the_pin_and_the_settling_host_follows_main() {
    let cfg: DocumentMut = HOSTS.parse().expect("fixture");
    assert!(pinned_by_settler(&cfg, "gex45") && pinned_by_settler(&cfg, "nexus"));
    assert!(!pinned_by_settler(&cfg, "whiterabbit"), "the settling host promotes the pin");
    assert!(!pinned_by_settler(&cfg, "laptop"), "an undeclared host has no route from the settling host");
    let alone: DocumentMut = "[hosts.gex45]\nssh = \"tom@gex45\"\n".parse().unwrap();
    assert!(!pinned_by_settler(&alone, "gex45"), "with no settling host nothing ships the pin");
}
