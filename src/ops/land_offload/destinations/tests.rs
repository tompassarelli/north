use super::deploy;
use crate::ops::pacing::placement::gate;
use toml_edit::DocumentMut;

#[test]
fn deploy_destinations_add_a_deploy_only_key_without_making_the_host_a_machine() {
    let cfg: DocumentMut = "[hosts.nexus]\nresident_cap = 6\ndeploy_ssh = \"tom@nexus\"\n[hosts.gex45]\nssh = \"tom@gex45\"\ndeploy_ssh = \"tom@gex45\"\n[hosts.laptop]\ndeploy_ssh = \"tom@laptop\"\n[hosts.bare]\nresident_cap = 1\n[hosts.copy]\nssh = \"tom@nexus\"\n".parse().unwrap();
    assert_eq!(deploy(&cfg, "laptop"), ["tom@nexus", "tom@gex45"]);
    assert_eq!(gate::machines(&cfg).iter().map(|machine| machine.name.as_str()).collect::<Vec<_>>(), ["gex45", "copy"]);
}

#[test]
fn a_host_builds_the_runtime_unless_it_declares_builds_runtime_false() {
    let cfg: DocumentMut = "[hosts.nexus]\nbuilds_runtime = false\n[hosts.gex45]\nssh = \"tom@gex45\"\n".parse().unwrap();
    assert!(!super::builds_runtime(&cfg, "nexus"));
    assert!(super::builds_runtime(&cfg, "gex45"));
    assert!(super::builds_runtime(&cfg, "unlisted"));
}
