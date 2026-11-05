use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use toml_edit::DocumentMut;

use super::ledgers_among;
use crate::land_offload;
use crate::pacing::placement::gate;

const HOSTS: &str = include_str!("../runtime/fixtures/orchestration-hosts.toml");
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
    let ledgers = ledgers_among(vec!["threads".into()], gate::machines(&cfg), "whiterabbit", "ssh", &BTreeMap::new());
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

