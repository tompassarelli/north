use super::harness::*;
use std::fs;
use std::path::Path;

#[test]
fn a_deploy_whose_remote_update_fails_agents_sync_names_the_step_and_its_redacted_sync_log() {
    let host = Host::new();
    host.built(&host.target);
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    script(&host.path("bin/rsync"), "true\n");
    script(
        &host.path("bin/ssh"),
        "while [[ $1 == -o ]]; do shift 2; done\n[[ $1 == -- ]] && shift\nshift\n[[ \"$2 $3\" == 'runtime update' ]] && AGENTS_ORCHESTRATION=\"$TEST_FAR_FLEET\" RESIDENT_CAP_HOST=plain exec \"$TEST_NORTH\" \"${@:2}\"\nexit 0\n",
    );
    let far_fleet = fleet(&host, "[hosts.plain]\nresident_cap = 1\n");
    let output = host.run(&["runtime", "deploy", "far"], 42, 0, &[("TEST_NORTH", env!("CARGO_BIN_EXE_north")), ("TEST_FAR_FLEET", far_fleet.to_str().unwrap())]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    let reported = stderr.lines().find(|line| line.starts_with("north runtime deploy: north runtime update --sha")).unwrap_or_else(|| panic!("{stderr}"));
    assert!(reported.contains("on far failed: exit status: 1; it said: "), "{reported}");
    assert!(reported.contains("north runtime update: agents sync log: fixture sync error: Authorization: [redacted]"), "{reported}");
    assert!(reported.contains(&format!("failed step agents sync on {}", host.target)), "{reported}");
    assert!(!stderr.contains(SECRET), "{stderr}");
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

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
