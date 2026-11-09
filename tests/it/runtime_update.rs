use std::fs::{self};
use std::path::Path;

#[path = "runtime_update/forward_only.rs"]
mod forward_only;
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
fn a_failed_sync_keeps_the_new_runtime_and_alerts_once_per_cause_without_the_secret() {
    for (alert_status, launched) in [(0, true), (43, true), (0, false)] {
        let host = skewed();
        if !launched {
            host.lose_agents();
        }
        let output = host.update(42, alert_status);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
        let alerts = fs::read_to_string(host.path("alerts")).unwrap_or_else(|_| panic!("no alert; stdout {} stderr {stderr}", String::from_utf8_lossy(&output.stdout)));
        let alert = alerts.lines().collect::<Vec<_>>();
        assert_eq!(alert[..5], ["fleet", "send", "--from", "service:north-runtime-update", "proxy"]);
        assert_eq!(alert.len(), 6, "{alerts}");
        let expected = format!("agents sync exited {}", if launched { 42 } else { 127 });
        assert!(alert[5].contains("the harness generation is not published") && alert[5].contains(&expected), "{}", alert[5]);
        assert!(!alert[5].contains(SECRET), "{}", alert[5]);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
        assert_eq!(stderr.contains("proxy alert delivery failed"), alert_status != 0, "{stderr}");
        let again = host.update(42, alert_status);
        assert!(again.status.success(), "{}", String::from_utf8_lossy(&again.stderr));
        assert_eq!(fs::read_to_string(host.path("alerts")).unwrap(), alerts, "the same cause alerts once");
    }
}

#[test]
fn a_promotion_that_cannot_root_a_store_path_leaves_the_current_runtime_selected() {
    let host = skewed();
    script(&host.runtime(&host.target).join("north"), "# /nix/store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84\n");
    host.stamp(&host.target);
    script(&host.path("bin/nix-store"), "exit 3\n");
    let output = host.update(0, 0);
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}


#[test]
fn a_runtime_whose_binary_was_stamped_for_another_commit_is_refused_and_the_current_runtime_stays() {
    let host = skewed();
    host.stamp(PREVIOUS);
    fs::copy(host.runtime(PREVIOUS).join("built-from"), host.runtime(&host.target).join("built-from")).unwrap();
    let output = host.update(0, 0);
    assert_ne!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

#[test]
fn a_runtime_built_before_the_built_from_record_existed_is_promoted_and_stamped() {
    let host = skewed();
    fs::remove_file(host.runtime(&host.target).join("built-from")).unwrap();
    fs::write(host.runtime(&host.target).join("built-here"), &host.target[..8]).unwrap();
    let output = host.update(0, 0);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
    assert!(host.runtime(&host.target).join("built-from").is_file());
}
