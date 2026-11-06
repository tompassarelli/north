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
