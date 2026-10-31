use super::*;

fn input(command: &str, timeout: Option<u64>, background: bool) -> HookInput {
    let mut tool_input = serde_json::json!({"command": command, "run_in_background": background});
    if let Some(ms) = timeout {
        tool_input["timeout"] = ms.into();
    }
    let payload = serde_json::json!({"tool_name": "Bash", "tool_input": tool_input});
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

#[test]
fn recorded_waits_over_a_minute_are_refused_and_bounded_waits_pass() {
    let cases: Vec<(String, Option<u64>, String)> =
        serde_json::from_str(include_str!("fixtures/poll_loop.json")).unwrap();
    for (command, timeout, expected) in cases {
        for source in [command.clone(), format!("cd /tmp && {command}")] {
            let bound = timeout.map_or(f64::INFINITY, |ms| ms as f64 / 1000.0);
            let observed = if decide_with(&source, bound, Path::new("")).is_some() { "deny" } else { "allow" };
            assert_eq!(observed, expected, "{source:?} timeout {timeout:?}");
        }
    }
}

#[test]
fn a_refusal_names_job_run_notify_and_monitor_and_a_background_loop_passes() {
    let refused = match verdict(&input("until test -f x; do sleep 5; done", None, false)) {
        Verdict::Deny(reason) => reason,
        other => panic!("{other:?}"),
    };
    assert!(refused.contains("job-run --notify") && refused.contains("Monitor"), "{refused}");
    assert!(matches!(verdict(&input("sleep 300", Some(120000), false)), Verdict::Deny(_)));
    assert!(matches!(verdict(&input("until test -f x; do sleep 5; done", None, true)), Verdict::Allow));
    assert!(matches!(verdict(&input("sleep 300", Some(30000), false)), Verdict::Allow));
}

#[test]
fn a_codex_exec_call_is_bounded_by_its_timeout_ms() {
    let call = |timeout: Option<u64>| {
        let mut tool_input = serde_json::json!({"cmd": "sleep 300"});
        if let Some(ms) = timeout {
            tool_input["timeout_ms"] = ms.into();
        }
        let payload = serde_json::json!({"tool_name": "exec_command", "tool_input": tool_input});
        verdict(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
    };
    assert!(matches!(call(None), Verdict::Deny(_)));
    assert!(matches!(call(Some(30000)), Verdict::Allow));
}

fn capacity_with(runs: &[(&str, i64, f64)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capacity.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE capacity_runs (at TEXT, shape TEXT, exitCode INTEGER, wallSeconds REAL)").unwrap();
    for (index, (shape, exit, seconds)) in runs.iter().enumerate() {
        db.execute(
            "INSERT INTO capacity_runs VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![format!("2026-10-28T00:00:{index:02}Z"), shape, exit, seconds],
        )
        .unwrap();
    }
    (dir, path)
}

#[test]
fn a_refusal_quotes_the_measured_p75_and_advises_by_it() {
    let (_dir, capacity) = capacity_with(&[
        ("cargo test", 0, 10.0),
        ("cargo test", 0, 20.0),
        ("cargo test", 0, 30.0),
        ("cargo test", 0, 40.0),
        ("cargo test", 1, 900.0),
        ("cargo build", 0, 100.0),
        ("cargo build", 0, 200.0),
        ("cargo build", 0, 300.0),
        ("cargo clippy", 0, 500.0),
        ("cargo clippy", 0, 500.0),
    ]);
    let refuse = |command: &str| decide_with(command, f64::INFINITY, &capacity).unwrap();
    let quick = refuse("cargo test && sleep 120 && cat out");
    assert!(quick.contains("`cargo test` has a measured p75 of 30 s over 4 runs") && quick.contains("wait inline"), "{quick}");
    let slow = refuse("until ! pgrep -f 'x'; do sleep 30; done; cargo build");
    assert!(slow.contains("`cargo build` has a measured p75 of 300 s over 3 runs") && slow.contains("end your turn"), "{slow}");
    let thin = refuse("cargo clippy; sleep 300");
    assert!(!thin.contains("p75") && thin.contains("job-run --notify"), "{thin}");
    assert!(!refuse("sleep 300").contains("p75"));
    assert!(decide_with("cargo test && sleep 20", f64::INFINITY, &capacity).is_none());
}
