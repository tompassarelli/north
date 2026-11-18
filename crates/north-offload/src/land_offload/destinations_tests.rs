use super::*;

const HOSTS: &str = include_str!("../../../north-runtime/src/runtime/fixtures/orchestration-hosts.toml");

#[test]
fn only_the_host_that_settles_the_fleet_publishes_the_facts_of_a_host_that_may_not_write_south() {
    let cfg: DocumentMut = HOSTS.parse().expect("fixture");
    assert!(!publishes_host_facts(&cfg, "nexus"));
    assert!(publishes_host_facts(&cfg, "gex45") && publishes_host_facts(&cfg, "whiterabbit"));
    assert_eq!(deploy(&cfg, "whiterabbit").relays_facts, [Relayed { destination: "nexus".into(), host: "nexus".into() }]);
    assert!(deploy(&cfg, "gex45").relays_facts.is_empty());
}

#[test]
fn an_offload_to_a_host_the_asker_does_not_reach_is_refused_before_any_dial() {
    let cfg: DocumentMut = HOSTS.parse().expect("fixture");
    assert_eq!(build_hosts(&cfg, "whiterabbit"), [("gex45".to_owned(), "tom@gex45".to_owned())]);
    assert!(build_hosts(&cfg, "gex45").is_empty());
    assert_eq!(reached_destination(&cfg, "whiterabbit", "gex45"), Ok("tom@gex45".to_owned()));
    let refused = reached_destination(&cfg, "gex45", "whiterabbit").expect_err("gex45 reaches no other host");
    assert!(refused.contains("whiterabbit") && refused.contains("this one reaches no other host"), "{refused}");
    let refused = reached_destination(&cfg, "whiterabbit", "elsewhere").expect_err("elsewhere is not declared");
    assert!(refused.contains("elsewhere") && refused.contains("this one reaches gex45"), "{refused}");
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
fn a_host_that_declares_an_agent_login_runs_offloaded_checks_as_the_operator_until_agent_offload_enables_it_north_1041() {
    let cfg: DocumentMut = "[hosts.far]\nssh = \"tom@far\"\nagent_ssh = \"agent@far\"\n\n[hosts.on]\nssh = \"tom@on\"\nagent_ssh = \"agent@on\"\nagent_offload = true\n\n[hosts.bare]\nssh = \"tom@bare\"\nagent_offload = true\n".parse().unwrap();
    let login = |name: &str| check_login(&cfg, &north_capacity::capacity::host_table::machines(&cfg).into_iter().find(|machine| machine.name == name).unwrap());
    assert_eq!(login("far").as_deref(), Some("tom@far"), "order 45 dialed agent@cpx62 for every check before the agent held a checkout there, and every check failed from 4:10 AM on 2026-11-13");
    assert_eq!(login("on").as_deref(), Some("agent@on"));
    assert_eq!(login("bare"), None, "a host set to run checks as its agent but naming no agent login must not run them as the operator");
}

#[test]
fn a_laptop_closed_by_its_host_switches_runs_no_check_disk_keep_or_build_ahead_there_north_1036_north_1362() {
    use crate::land_offload::room::{Room, Standing, check_host};
    use north_base::platform::orchestration::{builds_ahead, keeps_disk};
    let cfg: DocumentMut = "[hosts.laptop]\nruns_no_checks = true\noffload_builds = true\n\n[hosts.cpx62]\noffload_builds = true\n".parse().unwrap();
    assert!(runs_no_checks(&cfg, "laptop"));
    assert!(!runs_no_checks(&cfg, "cpx62"), "the switch defaults off");
    let paused: DocumentMut = "[hosts.whiterabbit]\ndisk_keep = false\nbuild_ahead = false\n".parse().unwrap();
    assert!(!keeps_disk(&paused, "whiterabbit") && !builds_ahead(&paused, "whiterabbit"), "the capacity tick starts no disk keep pass and the lander asks for no release north ahead there");
    assert!(keeps_disk(&cfg, "laptop") && builds_ahead(&cfg, "laptop") && keeps_disk(&paused, "cpx62"), "absent, both keys keep today's behavior");
    let closed = runs_no_checks(&cfg, "laptop");
    let standing = crate::land_offload::standing(closed, false, Some(&cfg), "laptop");
    assert_eq!(standing, Standing::Reserved, "offload_builds = true would make the laptop a Peer that runs a check itself once the remotes' queues pass a round");
    let here = || -> Option<Room> { panic!("a closed laptop reads and runs nothing itself") };
    assert!(check_host(standing, "laptop", here, vec![("cpx62".to_owned(), None)], &[], 6.0).is_err(), "an unreadable remote leaves the check unplaced, never on the laptop");
    assert!(check_host(standing, "laptop", here, Vec::new(), &[], 6.0).is_err());
    assert!(check_host(Standing::Fallback, "laptop", || None, Vec::new(), &[], 6.0).is_ok(), "unset, the laptop is the fallback that runs it");
}

#[test]
fn ssh_destination_from_orchestration_toml_refuses_an_ssh_option_before_ssh_runs() {
    assert_eq!(HOSTS.matches("ssh = \"tom@gex45\"").count(), 1);
    let cfg: DocumentMut = HOSTS.replace("ssh = \"tom@gex45\"", "ssh = \"-oProxyCommand=touch /tmp/p gex45\"").parse().expect("fixture");
    assert!(north_capacity::capacity::host_table::machines(&cfg).is_empty());
    assert_eq!(pull_sources(&cfg, "whiterabbit"), ["nexus"]);
}
