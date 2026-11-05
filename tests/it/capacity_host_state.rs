use crate::capacity_host;

use capacity_host::{Exclusive, Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Output;
use std::thread::sleep;
use std::time::Duration;

fn host() -> Host {
    Host::new("capacity-host-state-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)])
}

fn call(host: &Host, arguments: &[&str]) -> Output {
    host.helper(arguments).output_exclusive().unwrap()
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)))
}

fn field(host: &Host, arguments: &[&str], key: &str) -> Value {
    json(&call(host, arguments))[key].clone()
}

fn write_pressure(directory: &Path, avg10: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join("cpu.pressure"), format!("some avg10={avg10} avg60=0.00 avg300=0.00 total=0\n")).unwrap();
}

#[test]
fn a_reservation_can_be_renewed_and_released() {
    let host = host();
    let reserved = json(&call(&host, &["reserve", "--class", "agent", "--owner", "fixture:/capacity", "--timeout-seconds", "5"]));
    let lease = reserved["lease"].as_str().unwrap();
    assert!(lease.len() == 36 && lease.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-'), "{reserved}");
    let renewed = json(&call(&host, &["renew", "--lease", lease, "--owner", "fixture:/capacity", "--timeout-seconds", "5"]));
    assert_eq!(renewed["decision"], "RENEWED");
    let released = json(&call(&host, &["release", "--lease", lease, "--owner", "fixture:/capacity"]));
    assert_eq!(released["decision"], "RELEASED");
}

#[test]
fn a_batch_reservation_is_refused_with_exit_two_and_leaves_no_state() {
    let host = host();
    let runtime = host.path("rejected-runtime");
    let rejected = host.helper(&["reserve", "--class", "heavy", "--owner", "fixture:/capacity", "--timeout-seconds", "5"]).env("XDG_RUNTIME_DIR", &runtime).output_exclusive().unwrap();
    assert_eq!(rejected.status.code(), Some(2), "{}", String::from_utf8_lossy(&rejected.stderr));
    assert!(!runtime.join("agent-capacity-v1").exists());
}

#[test]
fn an_expired_reservation_is_reclaimed_by_the_next_probe() {
    let host = host();
    let expiring = json(&call(&host, &["reserve", "--class", "agent", "--owner", "fixture:/capacity", "--timeout-seconds", "1"]));
    assert_eq!(expiring["decision"], "RESERVED");
    sleep(Duration::from_millis(1100));
    assert_eq!(field(&host, &["probe", "--class", "agent"], "reclaimed"), 1);
}

#[test]
fn mode_selects_the_profile_and_auto_without_an_away_signal_is_attended() {
    let host = host();
    assert_eq!(field(&host, &["mode", "away"], "profile"), "unattended");
    assert_eq!(field(&host, &["probe", "--class", "agent"], "profile"), "unattended");
    call(&host, &["mode", "auto"]);
    assert_eq!(field(&host, &["probe", "--class", "agent"], "profile"), "attended");
    assert_eq!(field(&host, &["mode", "present"], "profile"), "attended");
}

fn throttled_lease_scope_cgroups(host: &Host) -> std::path::PathBuf {
    let user = uid();
    let manager = host.path(&format!("cgroup/user.slice/user-{user}.slice/user@{user}.service"));
    let lease_scope = manager.join("agent.slice/agent-capacity-lease.scope");
    for slice in [host.path("cgroup/system.slice"), manager.join("app.slice"), manager.join("background.slice"), manager.join("session.slice"), manager.join("native.slice")] {
        write_pressure(&slice, "0.00");
    }
    write_pressure(&manager.join("agent.slice"), "90.00");
    write_pressure(&lease_scope, "90.00");
    manager
}

#[test]
fn a_lease_scope_throttled_by_its_own_quota_does_not_hold_the_queue_but_other_slices_wait_does() {
    let host = host();
    let manager = throttled_lease_scope_cgroups(&host);
    let probe = |host: &Host| {
        let output = host.helper(&["probe", "--class", "moderate"]).env_remove("AGENT_CAPACITY_CPU_PRESSURE").output_exclusive().unwrap();
        let value = json(&output);
        value.get("reason").or_else(|| value.get("decision")).and_then(Value::as_str).unwrap_or_default().to_owned()
    };
    assert_ne!(probe(&host), "DEFER_CPU_PRESSURE");
    write_pressure(&manager.join("app.slice"), "90.00");
    assert_eq!(probe(&host), "DEFER_CPU_PRESSURE");
}

fn uid() -> u32 {
    let status = fs::read_to_string("/proc/self/status").unwrap();
    status.lines().find_map(|line| line.strip_prefix("Uid:")).and_then(|ids| ids.split_whitespace().next()).and_then(|id| id.parse().ok()).unwrap()
}

#[test]
fn a_deferred_native_client_holds_new_gpu_leases_until_it_is_admitted() {
    let host = host();
    call(&host, &["mode", "away"]);
    let reason = |class: &str| field(&host, &["probe", "--class", class], "reason");
    fs::write(host.path("gpu"), "95\n").unwrap();
    assert_eq!(reason("native"), "DEFER_GPU_BUSY");
    assert_eq!(reason("gpu"), "DEFER_NATIVE_WAITING");
    fs::write(host.path("gpu"), "0\n").unwrap();
    assert_eq!(reason("native"), "RUN");
    assert_eq!(reason("gpu"), "RUN");
}

#[test]
fn unleased_load_comes_from_the_watchdogs_latest_sample_and_commits_batch_cpus() {
    let host = host();
    throttled_lease_scope_cgroups(&host);
    let watchdog = host.path("watchdog");
    fs::create_dir_all(&watchdog).unwrap();
    let sample = r#"{"at":"1970-01-01T00:20:00.000Z","intervalSeconds":30,"cgroups":[{"cgroup":"app.slice","cores":2.5},{"cgroup":"app.slice/app-build.scope","cores":2},{"cgroup":"session.slice/niri.scope","cores":2},{"cgroup":"agent.slice","cores":4.6},{"cgroup":"agent.slice/agent-capacity.slice","cores":2},{"cgroup":"agent.slice/agent-capacity.slice/agent-capacity-0123abcd.scope","cores":2},{"cgroup":"agent.slice/run-p2-i2.scope","cores":0.6}],"top":[{"pid":7,"cores":2,"cgroup":"app.slice/app-build.scope","command":"make -j"}]}"#;
    fs::write(watchdog.join("samples.jsonl"), format!("{sample}\n")).unwrap();
    let at = |now: &str, arguments: &[&str]| {
        let output = host.helper(arguments).env_remove("AGENT_CAPACITY_CPU_PRESSURE").env("CAPACITY_WATCHDOG_DIR", &watchdog).env("AGENT_CAPACITY_NOW", now).output_exclusive().unwrap();
        json(&output)
    };
    assert_eq!(at("1300000", &["status"])["unleasedHeavy"], Value::Null);
    let unleased = &at("1210000", &["status"])["unleasedHeavy"];
    let heavy: Vec<(&str, f64)> = unleased["heavy"].as_array().unwrap().iter().map(|entry| (entry["cgroup"].as_str().unwrap(), entry["cores"].as_f64().unwrap())).collect();
    assert_eq!((unleased["cores"].as_f64(), unleased["windowSeconds"].as_u64(), heavy), (Some(4.6), Some(30), vec![("app.slice/app-build.scope", 2.0)]));
    let probe = at("1210000", &["probe", "--class", "agent"]);
    assert_eq!((probe["leasedBatchCpus"].as_f64(), probe["committedBatchCpus"].as_f64()), (Some(0.0), Some(4.6)));
}

#[test]
fn deadlines_are_capped_per_class_at_an_hour_for_batch_work_and_fifteen_minutes_exclusive() {
    let host = host();
    for (class, seconds) in [("exclusive", "901"), ("heavy", "3601")] {
        let refused = call(&host, &["run", "--class", class, "--owner", "fixture:/capacity", "--timeout-seconds", seconds, "--", "true"]);
        assert_eq!(refused.status.code(), Some(2), "{class} {seconds}: {}", String::from_utf8_lossy(&refused.stderr));
    }
}

#[test]
fn a_run_without_a_class_is_sized_from_its_programs_recorded_usage() {
    let host = host();
    let usage = host.path("sizing-usage.jsonl");
    let mut lines = String::new();
    for sample in 1..=3 {
        lines += &format!(r#"{{"at":"2026-10-10T00:00:0{sample}Z","owner":"fixture:/sized","shape":"fixturebuild","class":"moderate","wallSeconds":10,"meanCores":3,"p90Cores":4,"peakCores":4,"peakMemoryMiB":3200,"gpuSeconds":0}}"#);
        lines += "\n";
        lines += &format!(r#"{{"at":"2026-10-10T00:00:0{sample}Z","owner":"fixture:/sized","shape":"fixturerender","class":"moderate","wallSeconds":10,"meanCores":0.5,"p90Cores":0.6,"peakCores":0.6,"peakMemoryMiB":853,"gpuSeconds":6}}"#);
        lines += "\n";
    }
    fs::write(&usage, lines).unwrap();
    for program in ["fixturebuild", "fixturerender", "fixturenew"] {
        let path = host.path(program);
        north_core::foundation::shim::write_executable(&path, "#!/bin/sh\n");
    }
    let sized = |program: &str, database: &str, usage: &Path| {
        let path = host.path(program);
        let output = host
            .helper(&["run", "--owner", "fixture:/sized", "--timeout-seconds", "30", "--", path.to_str().unwrap()])
            .env("AGENT_CAPACITY_DB", host.path(database))
            .env("AGENT_CAPACITY_USAGE_LOG", usage)
            .output_exclusive()
            .unwrap();
        let first = String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or_default().to_owned();
        let value: Value = serde_json::from_str(&first).unwrap_or_else(|error| panic!("{error}: {first}"));
        (value["class"].clone(), value["requestedCpus"].clone(), value["requestedMemoryMiB"].clone())
    };
    assert_eq!(sized("fixturebuild", "sizing.db", &usage), ("heavy".into(), 4.into(), 3840.into()));
    assert_eq!(sized("fixturerender", "sizing.db", &usage), ("gpu".into(), 1.into(), 1024.into()));
    assert_eq!(sized("fixturenew", "unseen.db", &host.path("unseen-usage.jsonl")), ("moderate".into(), 2.into(), 2458.into()));
}
