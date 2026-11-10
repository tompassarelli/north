use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};
use std::time::Duration;

use north_base::lifecycle::{Fault, FaultKind};

use super::*;

const AT_MS: i64 = 1_789_000_000_000;

struct Fleet(Vec<(&'static str, String)>);

impl Host for Fleet {
    fn output(&self, argv: &[&str], _: Option<&Path>, _: Option<&str>, _: Duration) -> Option<Output> {
        let [.., destination, command] = argv else { return None };
        let (_, facts) = self.0.iter().find(|(name, _)| name == destination && *command == REMOTE_FACTS)?;
        Some(Output { status: ExitStatus::from_raw(0), stdout: facts.as_bytes().to_vec(), stderr: Vec::new() })
    }
}

fn refused_move(instance: &str) -> Fault {
    Fault { machine: "after-landing-run".into(), instance: instance.into(), state: "done".into(), since_ms: AT_MS, kind: FaultKind::Illegal { to: "retired".into(), at_ms: AT_MS }, cause: "the table forbids it".into() }
}

fn facts_json(host: &str, faults: Vec<Fault>) -> String {
    serde_json::to_string(&HostFacts { host: host.into(), lifecycles: Lifecycles::Read(faults), ..HostFacts::default() }).unwrap()
}

#[test]
fn a_refused_move_in_a_second_hosts_lifecycle_store_is_a_finding_for_that_host() {
    let root = tempfile::tempdir().unwrap();
    let policy = root.path().join("orchestration.toml");
    fs::write(&policy, "[hosts.laptop]\nssh = \"tom@laptop\"\n\n[hosts.gex45]\nssh = \"tom@gex45\"\n\n[hosts.cpx62]\nssh = \"tom@cpx62\"\n").unwrap();
    let fleet = Fleet(vec![
        ("tom@laptop", facts_json("laptop", vec![refused_move("local")])),
        ("tom@gex45", facts_json("gex45", vec![refused_move("17860e0a7f")])),
    ]);
    let faults: Vec<_> = remote_lifecycles(&fleet, &policy, "laptop").into_iter().flat_map(|(host, lifecycles)| host_faults(host, lifecycles)).collect();
    let findings = machine_refusal::refused(&machine_refusal::Facts { owner: None, faults });
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].key, format!("machine-refusal:after-landing-run:retired:{}", AT_MS / (7 * 24 * 3600 * 1000)));
    assert_eq!(findings[0].instances, vec!["gex45:17860e0a7f".to_owned()]);
    assert!(findings[0].detail.contains("on gex45 refused 1 move"), "{}", findings[0].detail);
}
