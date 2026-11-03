use super::super::Options;
use super::pull_among;
use crate::ops::pacing::placement::gate;
use std::process::Command;
use toml_edit::DocumentMut;

const HOSTS: &str = include_str!("../../../../crates/north-core/src/fixtures/orchestration-hosts.toml");
const FAKE_SSH: &str = "cd \"$(dirname \"$0\")\"\nfor word; do [ \"$word\" = -- ] && break; case \"$word\" in -oProxyCommand=*) sh -c \"${word#-oProxyCommand=}\" ;; esac; done\nexit 255\n";

fn options() -> Options {
    Options { session: String::new(), since: String::new(), backfill: false }
}

#[test]
fn ssh_destination_planted_in_orchestration_toml_never_reaches_the_worker_ledger_pull() {
    let root = tempfile::tempdir().unwrap();
    let ssh = root.path().join("ssh");
    crate::foundation::shim::write_script(&ssh, FAKE_SSH);
    let value = format!("ssh = \"-oProxyCommand=touch {}/p gex45\"", root.path().display());
    let cfg: DocumentMut = HOSTS.replace("ssh = \"tom@gex45\"", &value).parse().unwrap();
    let mut rows = Vec::new();
    pull_among(gate::machines(&cfg), "whiterabbit", ssh.to_str().unwrap(), &options(), b"{}", &mut rows);
    assert!(!root.path().join("p").exists());
    let unchecked = cfg["hosts"]["gex45"]["ssh"].as_str().unwrap().to_owned();
    let _ = Command::new(&ssh).args(["-o", "BatchMode=yes", &unchecked, "north", "worker-ledger", "--emit"]).status();
    assert!(root.path().join("p").exists(), "the fake ssh runs a ProxyCommand the way ssh does");
}

#[test]
fn ssh_destination_from_the_real_hosts_reaches_gex45_through_the_worker_ledger_pull() {
    let root = tempfile::tempdir().unwrap();
    let ssh = root.path().join("ssh");
    crate::foundation::shim::write_script(&ssh, &format!("printf '%s\\n' \"$*\" >> {}/calls\n", root.path().display()));
    let cfg: DocumentMut = HOSTS.parse().unwrap();
    let mut rows = Vec::new();
    pull_among(gate::machines(&cfg), "whiterabbit", ssh.to_str().unwrap(), &options(), b"{}", &mut rows);
    assert_eq!(std::fs::read_to_string(root.path().join("calls")).unwrap(), "-o BatchMode=yes -o ConnectTimeout=5 -- tom@gex45 north worker-ledger --emit\n");
}
