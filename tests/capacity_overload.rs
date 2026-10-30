mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::Stdio;

const PLACEMENT: &str = "[placement]\norder = [\"gex45\", \"anthropic-cloud\", \"nexus\", \"laptop\"]\n\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"native:wisp\", \"private:smashcraft\", \"native:smashcraft\", \"warcraft:smashcraft\"]\n\n[hosts.whiterabbit]\nserves = [\"private\", \"native\", \"warcraft\"]\n";

fn first_decision(host: &Host, need: &str) -> Value {
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    fs::write(host.path("orchestration.toml"), PLACEMENT).unwrap();
    let mut wrapper = Wrapper(
        host.helper(&["run", "--class", "heavy", "--owner", "fixture:/lagged", "--timeout-seconds", "60", "--", "true"])
            .env("AGENT_CAPACITY_NEED", need)
            .env("RESIDENT_CAP_HOST", "whiterabbit")
            .env("AGENTS_ORCHESTRATION", host.path("orchestration.toml"))
            .env("AGENTS_OPERATOR_DIR", host.path(""))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = BufReader::new(wrapper.0.stderr.take().unwrap());
    stderr.lines().map_while(Result::ok).filter_map(|line| serde_json::from_str::<Value>(&line).ok()).next().unwrap()
}

#[test]
fn a_heavy_run_from_a_native_smashcraft_box_above_the_thread_count_queues_naming_gex45() {
    let host = Host::new("capacity-overload-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    let line = first_decision(&host, "native:smashcraft");
    assert_eq!((line["decision"].as_str(), line["reason"].as_str(), line["offload"].as_str()), (Some("QUEUED"), Some("DEFER_RUN_QUEUE"), Some("gex45")), "{line}");
    let unserved = first_decision(&host, "private:galileo");
    assert_eq!((unserved["reason"].as_str(), unserved.get("offload")), (Some("DEFER_RUN_QUEUE"), None), "{unserved}");
}

#[test]
fn a_heavy_ticket_held_by_the_run_queue_holds_no_place_for_moderate_work() {
    let host = Host::new("capacity-overload-line-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    let _heavy = Wrapper(
        host.helper(&["run", "--class", "heavy", "--owner", "fixture:/held", "--timeout-seconds", "60", "--", "sleep", "60"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    host.until("the heavy ticket's run-queue reason", |host| host.status()["queued"].as_array().is_some_and(|queued| queued.iter().any(|ticket| ticket["reason"] == "DEFER_RUN_QUEUE")));
    let queued_ahead = |class: &str| {
        let output = host.helper(&["probe", "--class", class]).output().unwrap();
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["queuedAhead"].clone()
    };
    assert_eq!((queued_ahead("moderate"), queued_ahead("heavy")), (0.into(), 1.into()));
}
