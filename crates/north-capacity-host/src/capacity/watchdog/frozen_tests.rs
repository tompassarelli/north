use std::fs;

use super::frozen::sweep;

const MINUTE_MS: i64 = 60_000;

#[test]
fn a_session_scope_frozen_with_no_recorded_hold_is_thawed_after_five_minutes_and_a_held_one_is_left() {
    let root = tempfile::tempdir().unwrap();
    let cgroup = root.path().join("user@1000.service");
    let stray = cgroup.join("agent.slice/north-session-p88-threads.scope");
    let held = cgroup.join("agent.slice/north-session-held.scope");
    for scope in [&stray, &held] {
        fs::create_dir_all(scope).unwrap();
        fs::write(scope.join("cgroup.freeze"), "1\n").unwrap();
    }
    let holds = root.path().join("gaming-frozen.json");
    fs::write(&holds, r#"["north-session-held.scope"]"#).unwrap();
    let state = root.path().join("watchdog");
    let start = 1_793_000_000_000;
    assert!(sweep(&cgroup, &holds, &state, start).is_empty());
    assert!(sweep(&cgroup, &holds, &state, start + 5 * MINUTE_MS).is_empty(), "thawed at exactly five minutes");
    assert_eq!(sweep(&cgroup, &holds, &state, start + 5 * MINUTE_MS + 30_000), ["north-session-p88-threads.scope"]);
    assert_eq!(fs::read_to_string(stray.join("cgroup.freeze")).unwrap(), "0");
    assert_eq!(fs::read_to_string(held.join("cgroup.freeze")).unwrap().trim(), "1");
    let incidents = fs::read_to_string(state.join("incidents.md")).unwrap();
    assert!(incidents.contains("frozen-session-scope: north-session-p88-threads.scope stayed frozen 5 min with no recorded hold"), "{incidents}");
}
