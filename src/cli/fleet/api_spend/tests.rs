use super::far_hosts;

const HOSTS: &str = "[hosts.nexus]\ndeploy_ssh = \"nexus\"\n[hosts.gex45]\nssh = \"gex45\"\n[hosts.laptop]\nssh = \"laptop\"\n[hosts.cloud]\nresident_cap = 2\n[hosts.odd]\nssh = \"-oProxyCommand=x\"\n";

#[test]
fn a_host_declaring_only_deploy_ssh_is_pulled_and_this_host_and_unreachable_ones_are_not() {
    let cfg: toml_edit::DocumentMut = HOSTS.parse().unwrap();
    let names: Vec<String> = far_hosts(&cfg, "laptop").into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["nexus", "gex45"]);
}
