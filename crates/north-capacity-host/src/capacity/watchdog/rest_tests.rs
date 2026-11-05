use serde_json::json;

use super::rest::{Expected, Observed, check};

fn expected() -> Expected {
    serde_json::from_value(json!({
        "system": ["getty@*.service"], "user": ["niri.service", "worker-ledger.timer"],
        "sockets": ["tcp 127.0.0.1:7070 kanata-main.service", "udp *:* -"], "cores": 0.02, "budgets": ["niri.service 0.06"]
    }))
    .unwrap()
}

fn observed(units: serde_json::Value, sockets: serde_json::Value) -> Observed {
    serde_json::from_value(json!({ "seconds": 30, "units": units, "sockets": sockets })).unwrap()
}

fn agent() -> serde_json::Value {
    json!({ "scope": "user", "name": "app-niri-ghostty-1.scope", "cores": 0.3, "csPerSec": 150, "agent": "claude" })
}

#[test]
fn rest_check_fails_naming_extra_units_sockets_and_over_budget_units_and_lists_agents_apart() {
    let units = json!([
        { "scope": "user", "name": "niri.service", "cores": 0.05, "csPerSec": 9 }, { "scope": "user", "name": "worker-ledger.timer" },
        { "scope": "user", "name": "worker-ledger.service", "cores": 0.01, "csPerSec": 2 }, agent(),
        { "scope": "user", "name": "makgora-green-builds.timer" }, { "scope": "user", "name": "makgora-vm-retention.timer" },
        { "scope": "user", "name": "agent-capacity-presence.service", "cores": 0.01, "csPerSec": 29 },
        { "scope": "system", "name": "getty@tty1.service", "cores": 0.03, "csPerSec": 5 }
    ]);
    let sockets = json!(["tcp 127.0.0.1:7070 kanata-main.service", "udp 0.0.0.0:51820 -", "tcp 0.0.0.0:22 sshd.service"]);
    let verdict = check(&expected(), &observed(units, sockets));
    let lines = verdict.lines();
    assert!(!verdict.ok());
    let extra_units = lines.iter().filter(|line| line.starts_with("extra user unit: ") && ["makgora-green-builds.timer", "makgora-vm-retention.timer", "agent-capacity-presence.service"].iter().any(|name| line.contains(name))).count();
    assert_eq!(extra_units, 3, "{lines:#?}");
    assert!(lines.contains(&"extra socket: tcp 0.0.0.0:22 sshd.service".to_owned()), "{lines:#?}");
    assert!(lines.contains(&"over budget: system getty@tty1.service 0.03 cores > 0.02 (5 cs/s)".to_owned()), "{lines:#?}");
    assert!(lines.iter().any(|line| line.starts_with("agent: user app-niri-ghostty-1.scope")), "{lines:#?}");
    assert!(!lines.iter().any(|line| line.starts_with("extra") && ["ghostty", "niri", "worker-ledger", "51820"].iter().any(|name| line.contains(name))), "{lines:#?}");
}

#[test]
fn rest_check_passes_on_only_the_expected_set_plus_agents() {
    let units = json!([
        { "scope": "system", "name": "getty@tty1.service", "cores": 0, "csPerSec": 0 },
        { "scope": "user", "name": "niri.service", "cores": 0.05, "csPerSec": 9 }, { "scope": "user", "name": "worker-ledger.timer" },
        { "scope": "user", "name": "worker-ledger.service", "cores": 0.01, "csPerSec": 2 }, agent()
    ]);
    let sockets = json!(["tcp 127.0.0.1:7070 kanata-main.service", "udp 0.0.0.0:51820 -"]);
    let verdict = check(&expected(), &observed(units, sockets));
    assert!(verdict.ok());
    let summary = verdict.lines().pop().unwrap();
    assert_eq!(summary, "rest-check: at rest; 0 extra units, 0 extra sockets, 0 over budget, 1 agents; non-agent 0.06 cores, 11 cs/s");
}
