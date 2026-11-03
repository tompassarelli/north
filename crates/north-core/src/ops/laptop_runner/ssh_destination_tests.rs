use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use toml_edit::DocumentMut;

use super::ledgers_among;
use crate::ops::land_offload;
use crate::ops::pacing::placement::gate;

const HOSTS: &str = include_str!("../../fixtures/orchestration-hosts.toml");
const FAKE_SSH: &str = "cd \"$(dirname \"$0\")\"\nfor word; do [ \"$word\" = -- ] && break; case \"$word\" in -oProxyCommand=*) sh -c \"${word#-oProxyCommand=}\" ;; esac; done\nexit 255\n";

fn planted(root: &Path) -> DocumentMut {
    let value = format!("ssh = \"-oProxyCommand=touch {}/p gex45\"", root.display());
    assert_eq!(HOSTS.matches("ssh = \"tom@gex45\"").count(), 1);
    HOSTS.replace("ssh = \"tom@gex45\"", &value).parse().unwrap()
}

#[test]
fn ssh_destination_from_orchestration_toml_refuses_an_ssh_option_before_ssh_runs() {
    let root = tempfile::tempdir().unwrap();
    let ssh = root.path().join("ssh");
    crate::foundation::shim::write_script(&ssh, FAKE_SSH);
    let cfg = planted(root.path());
    assert!(gate::machines(&cfg).is_empty());
    assert_eq!(land_offload::pull_sources_in(&cfg, "whiterabbit"), ["nexus"]);
    let ledgers = ledgers_among(vec!["threads".into()], gate::machines(&cfg), "whiterabbit", &BTreeMap::new());
    assert_eq!(ledgers.len(), 1);
    assert!(ledgers[0].host.is_none());
    for ledger in ledgers.iter().filter(|ledger| ledger.host.is_some()) {
        let _ = Command::new(&ssh).args(&ledger.command[1..]).args(["list"]).status();
    }
    assert!(!root.path().join("p").exists());
    let unchecked = format!("-oProxyCommand=touch {}/p gex45", root.path().display());
    let _ = Command::new(&ssh).args(["-o", "BatchMode=yes", &unchecked, "north", "threads"]).status();
    assert!(root.path().join("p").exists(), "the fake ssh runs a ProxyCommand the way ssh does");
}

#[test]
fn ssh_destination_keeps_the_real_hosts_machine_and_deploy_destinations() {
    let cfg: DocumentMut = HOSTS.parse().unwrap();
    let machines = gate::machines(&cfg);
    assert_eq!(machines.iter().map(|machine| (machine.name.as_str(), machine.ssh.as_str())).collect::<Vec<_>>(), [("gex45", "tom@gex45")]);
    assert_eq!(land_offload::pull_sources_in(&cfg, "whiterabbit"), ["nexus", "tom@gex45"]);
    let ledgers = ledgers_among(vec!["threads".into()], machines, "whiterabbit", &BTreeMap::new());
    assert_eq!(ledgers[1].command, ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "--", "tom@gex45", "north", "threads"]);
}

#[test]
fn ssh_destination_accepts_only_a_plain_destination() {
    for refused in ["-oProxyCommand=id", "-p22", "", "tom@gex45 -oX", "a;b", "a$(id)", "tom@gex45\n"] {
        assert_eq!(gate::ssh_destination(refused), None, "{refused:?}");
    }
    for accepted in ["tom@gex45", "nexus", "host-1.example", "tom@10.77.0.1"] {
        assert_eq!(gate::ssh_destination(accepted), Some(accepted));
    }
}
