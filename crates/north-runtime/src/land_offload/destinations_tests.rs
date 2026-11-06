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
