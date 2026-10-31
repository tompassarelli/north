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

const OFFLOAD_HOSTS: &str = "[hosts.gex45]\noffload_builds = true\nssh = \"tom@gex45\"\n";

fn first_offload_line(host: &Host, need: &str) -> Value {
    first_line_with_hosts(host, need, OFFLOAD_HOSTS)
}

fn first_line_with_hosts(host: &Host, need: &str, hosts: &str) -> Value {
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    fs::write(host.path("orchestration.toml"), hosts).unwrap();
    let mut wrapper = Wrapper(
        host.helper(&["run", "--class", "heavy", "--owner", "fixture:/declined", "--timeout-seconds", "60", "--", "true"])
            .env("AGENT_CAPACITY_NEED", need)
            .env("RESIDENT_CAP_HOST", "laptop")
            .env_remove("NORTH_BUILD_OFFLOADED")
            .env_remove("SAFE_PUSH_LAND_HEAD")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = BufReader::new(wrapper.0.stderr.take().unwrap());
    stderr.lines().map_while(Result::ok).filter_map(|line| serde_json::from_str::<Value>(&line).ok()).next().unwrap()
}

#[test]
fn a_declined_offload_prints_one_line_naming_no_room_unreachable_or_the_refused_class() {
    let probe = "#!/bin/sh\nprintf '20\\n19.00 19.00 19.00 2/900 1\\nMemTotal: 65536000 kB\\nMemAvailable: 40000000 kB\\nFilesystem 1024-blocks Used Available Capacity Mounted on\\n/dev/md1 490000000 400000000 90000000 90%% /\\n'\n";
    let full = Host::new("capacity-offload-full-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL), ("ssh", probe)]);
    let line = first_offload_line(&full, "");
    assert_eq!((line["decision"].as_str(), line["reason"].as_str()), (Some("OFFLOAD_DECLINED"), Some("no room")), "{line}");
    assert!(line["detail"].as_str().is_some_and(|detail| detail.starts_with("gex45: ") && detail.contains("cores free of 20")), "{line}");
    let dark = Host::new("capacity-offload-dark-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL), ("ssh", "#!/bin/sh\nexit 255\n")]);
    let line = first_offload_line(&dark, "");
    assert_eq!((line["reason"].as_str(), line["detail"].as_str()), (Some("unreachable"), Some("gex45: unreachable or unreadable over ssh")), "{line}");
    let line = first_offload_line(&dark, "gpu");
    assert_eq!((line["decision"].as_str(), line["reason"].as_str()), (Some("OFFLOAD_DECLINED"), Some("refused: gpu")), "{line}");
    let line = first_line_with_hosts(&dark, "", "[hosts.gex45]\nssh = \"tom@gex45\"\n");
    assert_eq!(line["decision"].as_str(), Some("QUEUED"), "{line}");
}
