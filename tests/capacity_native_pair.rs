mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::process::Stdio;

fn host() -> Host {
    Host::new("capacity-native-pair-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)])
}

fn session(host: &Host, owner: &str, pair: &str, command: &[&str]) -> std::process::Output {
    let arguments = [&["session", "--class", "native", "--new-clients", "0", "--memory-gib", "1", "--owner", owner, "--pair", pair, "--"][..], command].concat();
    host.helper(&arguments).stdout(Stdio::piped()).output().unwrap()
}

#[test]
fn a_second_session_asking_for_a_pair_another_live_owner_holds_is_refused_naming_the_holder_and_gets_a_free_pair_instead() {
    let host = host();
    let holder_arguments = ["session", "--class", "native", "--new-clients", "0", "--memory-gib", "1", "--owner", "wisp-worker", "--pair", "4", "--", "sleep", "60"];
    let _holder = Wrapper(host.helper(&holder_arguments).stdout(Stdio::null()).spawn().unwrap());
    host.until("the pair 4 lease", |host| host.leases("native") == 1);
    assert_eq!(host.status()["holding"][0]["pair"], 4);

    let refused = session(&host, "sc-46-place", "4", &["true"]);
    assert_eq!(refused.status.code(), Some(75));
    let line: Value = serde_json::from_str(host.helper_stderr().lines().find(|line| line.contains("PAIR_HELD")).unwrap()).unwrap();
    assert_eq!((line["decision"].as_str(), line["pair"].as_u64(), line["holder"].as_str(), line["freePair"].as_u64()), (Some("REFUSED"), Some(4), Some("wisp-worker"), Some(0)), "{line}");
    assert_eq!(host.leases("native"), 1);
    assert!(!host.path("agent-capacity-v1/native-waiting").exists(), "a pair refusal waits for nothing");
    let holder_probe = host.helper(&["probe", "--class", "native", "--new-clients", "0", "--memory-gib", "1", "--pair", "4", "--owner", "wisp-worker"]).output().unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&holder_probe.stdout).unwrap()["decision"], "RUN");

    let free = session(&host, "sc-46-place", &line["freePair"].to_string(), &["true"]);
    assert!(free.status.success(), "{}", host.helper_stderr());
    let same_owner = session(&host, "wisp-worker", "4", &["true"]);
    assert!(same_owner.status.success(), "{}", host.helper_stderr());
}
