mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::Stdio;

const PLACEMENT: &str = "[placement]\norder = [\"gex45\", \"anthropic-cloud\", \"nexus\", \"laptop\"]\n\n[hosts.gex45]\nresident_cap = 24\nssh = \"tom@gex45\"\nserves = [\"native:wisp\", \"private:smashcraft\", \"native:smashcraft\", \"warcraft:smashcraft\"]\n\n[hosts.whiterabbit]\nserves = [\"private\", \"native\", \"warcraft\"]\n";

fn first_decision(host: &Host, need: &str) -> Value {
    fs::write(host.path("loadavg"), "34.68 29.11 23.93 31/2210 4096\n").unwrap();
    fs::write(host.path("orchestration.toml"), PLACEMENT).unwrap();
    let mut wrapper = Wrapper(
        host.helper(&["run", "--class", "heavy", "--owner", "fixture:/lagged", "--timeout-seconds", "60", "--", "true"])
            .env("AGENT_CAPACITY_LOADAVG", host.path("loadavg"))
            .env("AGENT_CAPACITY_NEED", need)
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
