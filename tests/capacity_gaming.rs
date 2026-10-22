mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::{Value, json};
use std::fs;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ONE_HEAVY_AND_ONE_MODERATE_CORES: &str = "11";

fn host() -> Host {
    Host::new("capacity-gaming-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)])
}

fn mark_gaming(host: &Host, gaming: bool) {
    let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    fs::write(host.path("agent-capacity-v1/gaming.json"), json!({ "gaming": gaming, "at": at }).to_string()).unwrap();
}

fn run(host: &Host, class: &str, owner: &str, command: &[&str]) -> Wrapper {
    let arguments = [&["run", "--class", class, "--owner", owner, "--timeout-seconds", "60", "--"][..], command].concat();
    Wrapper(host.helper(&arguments).env("AGENT_CAPACITY_CORES", ONE_HEAVY_AND_ONE_MODERATE_CORES).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap())
}

fn probe(host: &Host, class: &str) -> Value {
    let output = host.helper(&["probe", "--class", class]).env("AGENT_CAPACITY_CORES", ONE_HEAVY_AND_ONE_MODERATE_CORES).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)))
}

fn finished(host: &Host, what: &str, wrapper: &mut Wrapper) {
    let mut status = None;
    host.until(what, |_| {
        status = wrapper.exited();
        status.is_some()
    });
    assert!(status.unwrap().success(), "{what}: {}", wrapper.stderr());
}

#[test]
fn gaming_deferred_heavy_tickets_do_not_hold_moderate_work_and_keep_their_own_order() {
    let host = host();
    mark_gaming(&host, true);
    let mut first = run(&host, "heavy", "fixture:/first", &["true"]);
    host.until("the first heavy ticket", |host| host.queued("heavy") == 1);
    let _second = run(&host, "heavy", "fixture:/second", &["true"]);
    host.until("the second heavy ticket", |host| host.queued("heavy") == 2);

    let moderate = probe(&host, "moderate");
    assert_eq!(moderate["reason"], "RUN", "{moderate}");
    let heavy = probe(&host, "heavy");
    assert_eq!(heavy["reason"], "DEFER_GAMING", "{heavy}");
    assert_eq!(heavy["queuedAhead"], 2, "{heavy}");

    let mut light = run(&host, "moderate", "fixture:/light", &["true"]);
    finished(&host, "the moderate run", &mut light);
    let owners: Vec<_> = host.status()["queued"].as_array().unwrap().iter().map(|ticket| ticket["owner"].clone()).collect();
    assert_eq!(owners, [json!("fixture:/first"), json!("fixture:/second")]);
    let mut gpu = run(&host, "gpu", "fixture:/gpu", &["true"]);
    host.until("the gpu ticket", |host| host.queued("gpu") == 1);
    let still_moderate = probe(&host, "moderate");
    assert_eq!(still_moderate["reason"], "RUN", "{still_moderate}");

    mark_gaming(&host, false);
    finished(&host, "the first heavy run", &mut first);
    finished(&host, "the gpu run", &mut gpu);
}

#[test]
fn landing_heavy_check_queues_while_gaming_instead_of_taking_a_lease_to_be_frozen() {
    let host = host();
    mark_gaming(&host, true);
    let log = host.path("landing.log");
    let arguments = ["run", "--class", "heavy", "--owner", "land-check", "--timeout-seconds", "60", "--", "true"];
    let mut landing = Wrapper(
        host.helper(&arguments).env("SAFE_PUSH_LAND_HEAD", "fixture").env("AGENT_CAPACITY_CORES", ONE_HEAVY_AND_ONE_MODERATE_CORES).stdout(Stdio::null()).stderr(fs::File::create(&log).unwrap()).spawn().unwrap(),
    );
    let lines = |log: &std::path::Path| -> Vec<Value> {
        fs::read_to_string(log).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    };
    host.until("the landing check to report DEFER_GAMING", |_| lines(&log).iter().any(|line| line["reason"] == "DEFER_GAMING"));
    assert_eq!(host.queued("critical"), 1);
    assert_eq!(host.leases("heavy"), 0);
    assert!(landing.exited().is_none());

    let moderate = probe(&host, "moderate");
    assert_eq!(moderate["reason"], "RUN", "{moderate}");

    mark_gaming(&host, false);
    finished(&host, "the landing run", &mut landing);
    assert!(lines(&log).iter().any(|line| line["decision"] == "RESERVED"));
}

#[test]
fn only_a_fresh_gaming_marker_lets_moderate_work_pass_a_parked_heavy_ticket() {
    let host = host();
    let _holder = run(&host, "heavy", "fixture:/holder", &["sleep", "30"]);
    host.until("the heavy lease", |host| host.leases("heavy") == 1);
    let _parked = run(&host, "heavy", "fixture:/parked", &["true"]);
    host.until("the parked heavy ticket", |host| host.queued("heavy") == 1);

    let stale = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() - 300;
    fs::write(host.path("agent-capacity-v1/gaming.json"), json!({ "gaming": true, "at": stale }).to_string()).unwrap();
    let ignored = probe(&host, "moderate");
    assert_eq!(ignored["reason"], "DEFER_QUEUED", "{ignored}");

    mark_gaming(&host, true);
    let fresh = probe(&host, "moderate");
    assert_eq!(fresh["reason"], "RUN", "{fresh}");
}

const WALL_CLOCK_TIMEOUT: u64 = 2;
const FROZEN: Duration = Duration::from_secs(4);

fn wall_clock_systemd_run() -> String {
    let runtime_max = "for argument; do case $argument in --property=RuntimeMaxSec=*s) limit=${argument#--property=RuntimeMaxSec=}; (sleep \"${limit%s}\"; kill -KILL $$) >/dev/null 2>&1 & ;; esac; done\n";
    scope_systemd_run("\"$@\"").replacen("#!/bin/sh\n", &format!("#!/bin/sh\n{runtime_max}"), 1)
}

fn laptop_auto(host: &Host, gaming: bool) -> String {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    fs::write(host.path("playtest.json"), json!({ "on": gaming, "since": since }).to_string()).unwrap();
    let output = host
        .command(env!("CARGO_BIN_EXE_north"))
        .args(["capacity", "laptop-auto"])
        .env("AGENT_LOAD_PROC", host.path("proc"))
        .env("AGENT_LOAD_CGROUP", host.path("user-cgroup"))
        .env("AGENT_LOAD_DRM", host.path("no-drm"))
        .env("PLAYTEST_STATE", host.path("playtest.json"))
        .env("LAPTOP_AUTO_LAPTOP", "true")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn only_lease(host: &Host) -> Value {
    let entry = fs::read_dir(host.path("agent-capacity-v1/leases")).unwrap().flatten().find(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).unwrap();
    serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap()
}

fn at(milliseconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(milliseconds)
}

#[test]
fn a_lease_frozen_while_gaming_outlives_its_timeout_by_the_time_it_spent_frozen() {
    let host = Host::new("capacity-frozen-", &[("systemd-run", &wall_clock_systemd_run()), ("systemctl", SCOPE_SYSTEMCTL)]);
    let timeout = WALL_CLOCK_TIMEOUT.to_string();
    let arguments = ["run", "--class", "heavy", "--owner", "fixture:/frozen", "--timeout-seconds", &timeout, "--", "sleep", "60"];
    let mut frozen = Wrapper(host.helper(&arguments).env("AGENT_CAPACITY_CORES", ONE_HEAVY_AND_ONE_MODERATE_CORES).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    host.until("the heavy lease", |host| host.leases("heavy") == 1);
    let lease = only_lease(&host);
    let unit = format!("agent-capacity-{}.scope", lease["id"].as_str().unwrap().replace('-', ""));
    host.until("the lease scope", |host| host.path(&format!("state/scopes/{unit}")).exists());
    fs::create_dir_all(host.path(&format!("user-cgroup/agent.slice/agent-capacity.slice/{unit}"))).unwrap();
    fs::create_dir_all(host.path("proc")).unwrap();
    fs::write(host.path("proc/stat"), "btime 1\n").unwrap();
    let wrapper = lease["wrapperPid"].as_u64().unwrap();
    std::os::unix::fs::symlink(format!("/proc/{wrapper}"), host.path(&format!("proc/{wrapper}"))).unwrap();

    let froze = laptop_auto(&host, true);
    assert!(froze.contains(&format!("froze {{\"{unit}\"}}")), "{froze}");
    std::thread::sleep(FROZEN);
    assert!(frozen.exited().is_none(), "the frozen lease was killed by its wall-clock timeout");
    let thawed = laptop_auto(&host, false);
    assert!(thawed.contains(&format!("thawed {{\"{unit}\"}}")), "{thawed}");

    let paused = only_lease(&host)["expiresAt"].as_u64().unwrap() - lease["expiresAt"].as_u64().unwrap();
    let frozen_for = FROZEN.as_millis() as u64;
    assert!((frozen_for..frozen_for + 1000).contains(&paused), "paused {paused} ms for a {frozen_for} ms freeze");
    let created = lease["createdAt"].as_u64().unwrap();
    let original_plus_frozen = at(created + WALL_CLOCK_TIMEOUT * 1000 + frozen_for - 100);
    std::thread::sleep(original_plus_frozen.duration_since(SystemTime::now()).unwrap_or_default());
    assert!(frozen.exited().is_none(), "the lease died before its original timeout plus the time it was frozen");
    let deadline = at(created + WALL_CLOCK_TIMEOUT * 1000 + paused);
    let mut status = None;
    host.until("the deadline kill", |_| {
        status = frozen.exited();
        status.is_some()
    });
    let late = SystemTime::now().duration_since(deadline).unwrap_or_default();
    assert!(late < Duration::from_millis(1500), "killed {late:?} after the paused deadline");
    assert!(!status.unwrap().success());
    assert_eq!(host.leases("heavy"), 0);
}
