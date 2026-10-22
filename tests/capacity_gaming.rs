mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::{Value, json};
use std::fs;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

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
