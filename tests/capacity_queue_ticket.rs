mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::process::Stdio;

#[test]
fn an_auto_sized_run_queued_ahead_does_not_stop_a_later_caller_from_being_admitted() {
    let host = Host::new("capacity-queue-ticket-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    let _auto_sized = Wrapper(host.helper(&["run", "--owner", "fixture:/auto-sized", "--timeout-seconds", "30", "--", "sleep", "60"]).stdout(Stdio::null()).spawn().unwrap());
    host.until("the auto-sized run's queue ticket", |host| fs::read_dir(host.path("agent-capacity-v1/queue")).is_ok_and(|tickets| tickets.count() == 1));

    let output = host.helper(&["probe", "--class", "agent"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let probe: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(probe["queuedAhead"], 1);
}
