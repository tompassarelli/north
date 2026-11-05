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
fn a_monitor_until_loop_passes_because_its_events_arrive_without_holding_a_request() {
    let monitor = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "until [ -s out ]; do sleep 5; done; echo done", "description": "run finished", "timeout_ms": 1800000},
    });
    assert!(matches!(verdict(&HookInput::parse(monitor.to_string().as_bytes()).unwrap()), Verdict::Allow));
    assert!(matches!(verdict(&input("until [ -s out ]; do sleep 5; done", Some(1800000), false)), Verdict::Deny(_)));
}
