use super::harness::*;
use std::fs;
use std::path::Path;

#[test]
fn an_agents_sync_that_times_out_once_is_retried_and_the_runtime_promoted_without_a_hold() {
    let host = Host::new();
    host.built(&host.target);
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    script(&host.path("bin/agents"), "[[ -e $TEST_SYNCED ]] && exit 0\ntouch \"$TEST_SYNCED\"\nexec sleep 30\n");
    let output = host.north_with(&["runtime", "update"], &[("NORTH_RUNTIME_STEP_SECONDS", "1")]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
    assert!(!host.path("state/north/runtime-failed").exists() || !fs::read_to_string(host.path("state/north/runtime-failed")).unwrap().contains(&host.target));
    assert!(!host.path("alerts").exists());
}
