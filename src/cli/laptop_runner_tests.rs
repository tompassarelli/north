use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::laptop_runner::{Inputs, drain, ledgers};
use toml_edit::DocumentMut;

const ORCHESTRATION_HOSTS: &str = r#"
[hosts.nexus]
resident_cap = 6
deploy_ssh = "nexus"
builds_runtime = false

[hosts.gex45]
resident_cap = 24
pulls_spool = true
warcraft_clients = 6
lease_builds = true
offload_builds = true
ssh = "tom@gex45"
serves = ["native:wisp", "warcraft:wisp", "wc3:wisp", "client:wisp", "private:makgora", "native:makgora", "warcraft:makgora", "wc3:makgora", "client:makgora"]

[hosts.gex45.disk_budget_gib]
cargo_target = 20
map_store = 8
makgora_evidence = 15
mutant_cargo = 8
capture_batches = 8

[hosts.whiterabbit]
resident_cap = 8
settles_fleet = true
serves = ["private", "native", "warcraft", "wc3", "client", "sign-ins", "local-state", "tui", "fleet", "live", "desktop", "laptop"]

[placement]
order = ["gex45", "anthropic-cloud", "nexus", "laptop", "codex-cloud"]
"#;

fn script(path: &Path, body: &str) {
    north_core::foundation::shim::write_script(path, body);
}

fn planted(root: &Path, job_source: bool) -> (String, bool) {
    let marker = root.join("q");
    let job = serde_json::json!({"id": 25, "cmd": ["touch", marker.display().to_string()]}).to_string();
    fs::write(root.join("gex45-queue.txt"), format!("{job}\n")).unwrap();
    script(&root.join("ssh"), &format!("echo \"$*\" >> {0}/ssh.log\ncase \"$*\" in *\"take\"*) head -n1 {0}/gex45-queue.txt; sed -i 1d {0}/gex45-queue.txt ;; esac\n", root.display()));
    script(&root.join("threads"), "exit 0\n");
    script(&root.join("north"), "[ \"$1\" = capacity ] && exit 1\n[ \"$1\" = job-run ] && { shift 3; exec \"$@\"; }\necho '{\"state\":\"available\"}'\n");
    let mut text = ORCHESTRATION_HOSTS.to_owned();
    if job_source {
        text = text.replace("ssh = \"tom@gex45\"", "ssh = \"tom@gex45\"\njob_source = true");
    }
    let cfg: DocumentMut = text.parse().unwrap();
    let env = BTreeMap::from([("HOME".to_owned(), root.display().to_string())]);
    let inputs = Inputs {
        placement: "laptop".into(),
        poll: Duration::from_millis(100),
        runtime: root.to_owned(),
        logs: root.join("logs"),
        power_supply: root.join("power"),
        ledgers: ledgers(vec![root.join("threads").display().to_string()], Some(&cfg), "whiterabbit", &root.join("ssh").display().to_string(), &env),
        north: root.join("north").display().to_string(),
    };
    assert_eq!(drain(&inputs), 0);
    (fs::read_to_string(root.join("ssh.log")).unwrap_or_default(), marker.exists())
}

#[test]
fn a_laptop_job_planted_in_gex45s_ledger_is_never_taken_unless_config_names_gex45_a_job_source() {
    let closed = tempfile::tempdir().unwrap();
    let (polled, ran) = planted(closed.path(), false);
    assert!(!polled.contains("tom@gex45 north threads"), "{polled}");
    assert!(!ran, "south's orchestration.toml sets nothing for gex45, so its ledger is never drained");
    let open = tempfile::tempdir().unwrap();
    let (polled, ran) = planted(open.path(), true);
    assert!(polled.contains("tom@gex45 north threads"), "{polled}");
    assert!(ran, "with job_source = true the planted job runs, so the closed case is not a broken fixture");
}
