use std::fs::{self};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[path = "runtime_update/harness.rs"]
mod harness;
#[path = "runtime_update/prune.rs"]
mod prune;
#[path = "runtime_update/sync_step.rs"]
mod sync_step;

use harness::*;

fn skewed() -> Host {
    let host = Host::new();
    host.built(&host.target);
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    host
}

#[test]
fn a_failed_sync_restores_the_previous_runtime_and_alerts_once_without_the_log() {
    for (alert_status, launched) in [(0, true), (43, true), (0, false)] {
        let host = skewed();
        if !launched {
            host.lose_agents();
        }
        let output = host.update(42, alert_status);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
        let alerts = fs::read_to_string(host.path("alerts")).unwrap();
        let alert = alerts.lines().collect::<Vec<_>>();
        assert_eq!(alert[..5], ["fleet", "send", "--from", "service:north-runtime-update", "proxy"]);
        assert_eq!(alert.len(), 6, "{alerts}");
        let expected = format!("failed step agents sync on {}; rolled back to runtimes/{PREVIOUS}; exit status {}; local log: ", host.target, if launched { 42 } else { 127 });
        assert!(alert[5].contains(&expected), "{}", alert[5]);
        assert!(!alert[5].contains(SECRET) && !alert[5].contains("fixture sync error"));
        let log = alert[5].rsplit("local log: ").next().unwrap();
        assert_eq!(fs::metadata(log).unwrap().permissions().mode() & 0o777, 0o600);
        let logged = if launched { SECRET } else { "agents sync did not run" };
        assert!(fs::read_to_string(log).unwrap().contains(logged));
        assert_eq!(stderr.contains("rollback alert delivery failed"), alert_status != 0, "{stderr}");
    }
}

#[test]
fn a_promotion_that_cannot_root_a_store_path_leaves_the_current_runtime_selected() {
    let host = skewed();
    script(&host.runtime(&host.target).join("north"), "# /nix/store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84\n");
    script(&host.path("bin/nix-store"), "exit 3\n");
    let output = host.update(0, 0);
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

