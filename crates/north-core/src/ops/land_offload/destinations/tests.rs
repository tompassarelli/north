use super::{Deployment, deploy};
use crate::ops::pacing::placement::gate;
use toml_edit::DocumentMut;

#[test]
fn deploy_destinations_add_a_deploy_only_key_without_making_the_host_a_machine() {
    let cfg: DocumentMut = "[hosts.nexus]\nresident_cap = 6\ndeploy_ssh = \"tom@nexus\"\n[hosts.gex45]\nssh = \"tom@gex45\"\ndeploy_ssh = \"tom@gex45\"\n[hosts.laptop]\ndeploy_ssh = \"tom@laptop\"\n[hosts.bare]\nresident_cap = 1\n[hosts.copy]\nssh = \"tom@nexus\"\n".parse().unwrap();
    assert_eq!(deploy(&cfg, "laptop").destinations, ["tom@nexus", "tom@gex45"]);
    assert_eq!(gate::machines(&cfg).iter().map(|machine| machine.name.as_str()).collect::<Vec<_>>(), ["gex45", "copy"]);
}

#[test]
fn a_host_builds_the_runtime_unless_it_declares_builds_runtime_false() {
    let cfg: DocumentMut = "[hosts.nexus]\nbuilds_runtime = false\n[hosts.gex45]\nssh = \"tom@gex45\"\n".parse().unwrap();
    assert!(!super::builds_runtime(&cfg, "nexus"));
    assert!(super::builds_runtime(&cfg, "gex45"));
    assert!(super::builds_runtime(&cfg, "unlisted"));
}

#[test]
fn capacity_only_a_host_with_offload_builds_and_an_ssh_destination_takes_other_hosts_builds() {
    let cfg: DocumentMut = "[hosts.gex45]\nssh = \"tom@gex45\"\noffload_builds = true\n[hosts.nexus]\nssh = \"tom@nexus\"\n[hosts.laptop]\nssh = \"tom@laptop\"\noffload_builds = true\n[hosts.bare]\noffload_builds = true\n".parse().unwrap();
    assert_eq!(super::build_hosts(&cfg, "laptop"), [("gex45".to_owned(), "tom@gex45".to_owned())]);
}

#[test]
fn a_host_that_does_not_settle_the_fleet_leaves_a_host_that_builds_no_runtime_out_of_its_deploy_and_says_so() {
    let cfg: DocumentMut = "[hosts.nexus]\ndeploy_ssh = \"nexus\"\nbuilds_runtime = false\n[hosts.gex45]\nssh = \"tom@gex45\"\n[hosts.laptop]\nssh = \"tom@laptop\"\nsettles_fleet = true\n".parse().unwrap();
    let from_gex45 = deploy(&cfg, "gex45");
    assert_eq!(from_gex45, Deployment { destinations: vec!["tom@laptop".into()], skipped: vec!["nexus".into()] });
    assert_eq!(from_gex45.skip_line().as_deref(), Some("north runtime deploy: skipping nexus: it builds no runtime, and the host that settles the fleet ships to it"));
    let from_laptop = deploy(&cfg, "laptop");
    assert_eq!(from_laptop, Deployment { destinations: vec!["nexus".into(), "tom@gex45".into()], skipped: Vec::new() });
    assert_eq!(from_laptop.skip_line(), None);
}
