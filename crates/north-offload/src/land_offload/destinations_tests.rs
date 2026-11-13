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
fn a_host_the_settling_host_ships_to_runs_the_pin_and_the_settling_host_follows_main() {
    let cfg: DocumentMut = HOSTS.parse().expect("fixture");
    assert!(pinned_by_settler(&cfg, "gex45") && pinned_by_settler(&cfg, "nexus"));
    assert!(!pinned_by_settler(&cfg, "whiterabbit"), "the settling host promotes the pin");
    assert!(!pinned_by_settler(&cfg, "laptop"), "an undeclared host has no route from the settling host");
    let alone: DocumentMut = "[hosts.gex45]\nssh = \"tom@gex45\"\n".parse().unwrap();
    assert!(!pinned_by_settler(&alone, "gex45"), "with no settling host nothing ships the pin");
    let missing = tempfile::tempdir().unwrap().path().join("agents/orchestration.toml");
    let unread = || Err("not found".to_owned());
    assert_eq!(settled_elsewhere_read(Ok(cfg), "gex45", &missing, true), Ok(true));
    assert_eq!(settled_elsewhere_read(unread(), "gex45", &missing, false), Ok(false), "a home with no south declares no fleet");
    let refused = settled_elsewhere_read(unread(), "gex45", &missing, true).expect_err("a south kept without its orchestration file is no answer");
    assert!(refused.contains("orchestration.toml"), "{refused}");
}

#[test]
fn a_host_with_offload_builds_false_is_left_out_of_the_survey_at_every_standing_of_the_sender() {
    use crate::land_offload::room::Standing;
    let cfg: DocumentMut = "[hosts.quiet]\noffload_builds = false\n\n[hosts.busy]\noffload_builds = true\n".parse().unwrap();
    for standing in [Standing::Peer, Standing::Fallback, Standing::Reserved] {
        assert!(!takes_land_checks(&cfg, standing, "quiet"), "{standing:?}");
        assert!(takes_land_checks(&cfg, standing, "busy"), "{standing:?}");
    }
}

#[test]
fn a_host_with_offload_builds_false_warms_no_slot() {
    let cfg: DocumentMut = "[hosts.quiet]\noffload_builds = false\n\n[hosts.busy]\noffload_builds = true\n".parse().unwrap();
    assert!(!warms_slots(&cfg, "quiet"));
    assert!(warms_slots(&cfg, "busy"));
}

#[test]
fn a_host_that_declares_an_agent_login_runs_offloaded_checks_as_the_operator_until_agent_offload_enables_it_north_1041() {
    let cfg: DocumentMut = "[hosts.far]\nssh = \"tom@far\"\nagent_ssh = \"agent@far\"\n\n[hosts.on]\nssh = \"tom@on\"\nagent_ssh = \"agent@on\"\nagent_offload = true\n\n[hosts.bare]\nssh = \"tom@bare\"\nagent_offload = true\n".parse().unwrap();
    let login = |name: &str| check_login(&cfg, &north_pacing::pacing::placement::gate::machines(&cfg).into_iter().find(|machine| machine.name == name).unwrap());
    assert_eq!(login("far").as_deref(), Some("tom@far"), "order 45 dialed agent@cpx62 for every check before the agent held a checkout there, and every check failed from 4:10 AM on 2026-11-13");
    assert_eq!(login("on").as_deref(), Some("agent@on"));
    assert_eq!(login("bare"), None, "a host set to run checks as its agent but naming no agent login must not run them as the operator");
}

#[test]
fn a_laptop_closed_by_runs_no_checks_sends_every_check_to_a_remote_or_queues_it_there_never_here_north_1036() {
    use crate::land_offload::room::{Room, Standing, check_host};
    let cfg: DocumentMut = "[hosts.laptop]\nruns_no_checks = true\noffload_builds = true\n\n[hosts.cpx62]\noffload_builds = true\n".parse().unwrap();
    assert!(runs_no_checks(&cfg, "laptop"));
    assert!(!runs_no_checks(&cfg, "cpx62"), "the switch defaults off");
    let closed = runs_no_checks(&cfg, "laptop");
    let standing = crate::land_offload::standing(closed, false, Some(&cfg), "laptop");
    assert_eq!(standing, Standing::Reserved, "offload_builds = true would make the laptop a Peer that runs a check itself once the remotes' queues pass a round");
    let here = || -> Option<Room> { panic!("a closed laptop reads and runs nothing itself") };
    assert!(check_host(standing, "laptop", here, vec![("cpx62".to_owned(), None)], &[], 6.0).is_err(), "an unreadable remote leaves the check unplaced, never on the laptop");
    assert!(check_host(standing, "laptop", here, Vec::new(), &[], 6.0).is_err());
    assert!(check_host(Standing::Fallback, "laptop", || None, Vec::new(), &[], 6.0).is_ok(), "unset, the laptop is the fallback that runs it");
}
