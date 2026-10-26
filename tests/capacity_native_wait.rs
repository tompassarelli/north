mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::process::Stdio;

fn host() -> Host {
    Host::new("capacity-native-wait-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)])
}

fn native_start(host: &Host, owner: &str, wait_seconds: &str, command: &[&str]) -> Wrapper {
    let arguments = [&["session", "--class", "native", "--memory-gib", "1", "--wait-seconds", wait_seconds, "--owner", owner, "--"][..], command].concat();
    Wrapper(host.helper(&arguments).stdout(Stdio::null()).spawn().unwrap())
}

fn owners(entries: &Value) -> Vec<&str> {
    entries.as_array().unwrap().iter().filter(|entry| entry["class"] == "native").map(|entry| entry["owner"].as_str().unwrap()).collect()
}

#[test]
fn a_deferred_native_start_keeps_its_place_so_a_later_start_waits_behind_it() {
    let host = host();
    fs::write(host.path("gpu"), "95\n").unwrap();
    let _first = native_start(&host, "pair-0", "60", &["sleep", "60"]);
    host.until("pair 0's queue ticket", |host| host.queued("native") == 1);
    let _second = native_start(&host, "pair-1", "60", &["sleep", "60"]);
    host.until("pair 1 to queue behind pair 0", |host| host.helper_stderr().contains(r#""decision":"QUEUED","reason":"DEFER_QUEUED""#));
    assert_eq!(owners(&host.status()["queued"]), ["pair-0", "pair-1"]);
    let probe: Value = serde_json::from_slice(&host.helper(&["probe", "--class", "native", "--memory-gib", "1"]).output().unwrap().stdout).unwrap();
    assert_eq!(probe["reason"], "DEFER_QUEUED");

    fs::write(host.path("gpu"), "0\n").unwrap();
    host.until("both native leases", |host| host.leases("native") == 2);
    assert_eq!(owners(&host.status()["holding"]), ["pair-0", "pair-1"]);
}

#[test]
fn a_native_start_that_waits_past_its_seconds_exits_75_with_its_last_deferral() {
    let host = host();
    fs::write(host.path("gpu"), "95\n").unwrap();
    let mut waiting = native_start(&host, "pair-2", "1", &["true"]);
    let mut status = None;
    host.until("the wait to end", |_| {
        status = waiting.exited();
        status.is_some()
    });
    assert_eq!(status.unwrap().code(), Some(75));
    let last: Value = serde_json::from_str(host.helper_stderr().lines().last().unwrap()).unwrap();
    assert_eq!((last["decision"].as_str(), last["reason"].as_str()), (Some("DEFER"), Some("DEFER_GPU_BUSY")));
    assert_eq!(host.queued("native"), 0);
}
