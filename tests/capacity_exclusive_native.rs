mod capacity_host;

use capacity_host::{Host, Wrapper};
use serde_json::Value;
use std::fs;
use std::process::Stdio;

const SYSTEMD_RUN: &str = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";
const SYSTEMCTL: &str = "#!/bin/sh\ncase \" $* \" in *\" show \"*) echo inactive ;; esac\n";

fn timed(host: &Host, name: &str) -> String {
    let (running, overlap) = (host.path("running"), host.path("overlap"));
    format!(
        "mkdir {running} 2>/dev/null || echo {name} >>{overlap}; sleep 1; rmdir {running} 2>/dev/null",
        running = running.display(),
        overlap = overlap.display(),
    )
}

fn host() -> Host {
    let host = Host::new("exclusive-native-", &[("systemd-run", SYSTEMD_RUN), ("systemctl", SYSTEMCTL)]);
    fs::write(host.path("meminfo"), "MemTotal:       98655232 kB\nMemAvailable:   81920000 kB\n").unwrap();
    host
}

fn helper(host: &Host, arguments: &[&str]) -> Wrapper {
    let mut command = host.helper(arguments);
    command.env("AGENT_CAPACITY_MEMINFO", host.path("meminfo")).stdout(Stdio::null()).stderr(Stdio::piped());
    Wrapper(command.spawn().unwrap())
}

fn exclusive(host: &Host) -> Wrapper {
    let script = timed(host, "exclusive");
    let command = ["run", "--class", "exclusive", "--owner", "sc-239-laptop3:pair-1", "--timeout-seconds", "60", "--", "sh", "-c", &script];
    helper(host, &command)
}

fn native(host: &Host) -> Wrapper {
    let script = timed(host, "native");
    let command = ["session", "--class", "native", "--new-clients", "0", "--quiet", "--owner", "wisp-38-rerun:r200-l2-j20-light", "--", "sh", "-c", &script];
    helper(host, &command)
}

fn settle(wrapper: &mut Wrapper) -> Vec<Value> {
    while wrapper.exited().is_none() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    wrapper.stderr().lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect()
}

fn reserved(lines: &[Value]) -> bool {
    lines.iter().any(|line| line["decision"] == "RESERVED")
}

#[test]
fn exclusive_lease_and_native_timing_session_started_in_the_same_instant_never_run_together() {
    for round in 0..4 {
        let host = host();
        let (mut first, mut second) = if round % 2 == 0 { (exclusive(&host), native(&host)) } else { (native(&host), exclusive(&host)) };
        let (first, second) = (settle(&mut first), settle(&mut second));
        assert!(!fs::exists(host.path("overlap")).unwrap(), "round {round}: both ran at once: {first:?} {second:?}");
        assert!(reserved(&first) || reserved(&second), "round {round}: neither was admitted: {first:?} {second:?}");
    }

    let host = host();
    let mut held = exclusive(&host);
    host.until("the exclusive lease", |host| host.leases("exclusive") == 1);
    let refused = settle(&mut native(&host));
    assert!(!reserved(&refused), "a native session was admitted beside a live exclusive lease: {refused:?}");
    assert_eq!(refused.last().unwrap()["reason"], "DEFER_EXCLUSIVE", "{refused:?}");
    assert!(reserved(&settle(&mut held)));
    assert!(!fs::exists(host.path("overlap")).unwrap());
}

#[test]
fn quiet_native_run_inside_its_own_exclusive_lease_is_admitted() {
    let host = host();
    let bundle = Host::bundle();
    let inner = ["session", "--class", "native", "--new-clients", "0", "--quiet", "--owner", "sc-239-laptop3:pair-1-capture", "--", "true"];
    let command = [&["run", "--class", "exclusive", "--owner", "sc-239-laptop3:pair-1", "--timeout-seconds", "60", "--", "bun", bundle.to_str().unwrap()][..], &inner].concat();
    let lines = settle(&mut helper(&host, &command));
    let decisions: Vec<_> = lines.iter().map(|line| (line["class"].clone(), line["decision"].clone())).collect();
    assert!(lines.iter().any(|line| line["class"] == "exclusive" && line["decision"] == "RESERVED"), "{decisions:?}");
    let nested = lines.iter().find(|line| line["class"] == "native").expect("a native decision line");
    assert_eq!(nested["decision"], "RESERVED", "{nested}");
}
