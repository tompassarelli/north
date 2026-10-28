mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::{Value, json};
use std::fs;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

fn reserved_host(available_kib: u64) -> Host {
    let host = Host::new("capacity-game-reserve-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    fs::write(host.path("agent-capacity-v1/gaming.json"), json!({ "gaming": true, "at": at }).to_string()).unwrap();
    fs::write(host.path("game-reserve.json"), json!({ "on": true, "auto": true, "since": at }).to_string()).unwrap();
    fs::write(host.path("meminfo"), format!("MemTotal:       98655232 kB\nMemAvailable:   {available_kib} kB\n")).unwrap();
    host
}

fn probe(host: &Host, class: &str) -> Value {
    let output = host.helper(&["probe", "--class", class]).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)))
}

#[test]
fn game_reserve_admits_heavy_batch_on_the_agent_cpus_and_keeps_native_and_gpu_deferred() {
    let host = reserved_host(71_979_008);
    for class in ["heavy", "moderate"] {
        let admission = probe(&host, class);
        assert_eq!(admission["reason"], "RUN", "{admission}");
    }
    for class in ["native", "gpu", "exclusive"] {
        let admission = probe(&host, class);
        assert_eq!(admission["reason"], "DEFER_GAMING", "{admission}");
    }
    let mut heavy = Wrapper(host.helper(&["run", "--class", "heavy", "--owner", "fixture:/heavy", "--timeout-seconds", "60", "--", "true"]).stdout(Stdio::null()).spawn().unwrap());
    let mut status = None;
    host.until("the heavy run beside the game", |_| {
        status = heavy.exited();
        status.is_some()
    });
    assert!(status.unwrap().success(), "{}", host.helper_stderr());
}

#[test]
fn game_reserve_defers_batch_that_would_leave_the_game_under_its_memory_headroom() {
    let host = reserved_host(30_720_000);
    let heavy = probe(&host, "heavy");
    assert_eq!(heavy["reason"], "DEFER_GAME_MEMORY", "{heavy}");
    let moderate = probe(&host, "moderate");
    assert_eq!(moderate["reason"], "RUN", "{moderate}");
}
