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
            let observed = if decide(&source, bound).is_some() { "deny" } else { "allow" };
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
