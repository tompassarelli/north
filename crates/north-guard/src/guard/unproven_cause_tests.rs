#![cfg(test)]

use super::unproven_cause::{decide, unproven_cause};
use crate::hook::{HookInput, Verdict};
use serde_json::json;

fn stop(reply: &str, session: &str) -> HookInput {
    let transcript = format!("{}/src/guard/fixtures/recommend_acts/acted.jsonl", env!("CARGO_MANIFEST_DIR"));
    let data = json!({"hook_event_name":"Stop", "agent_type":"proxy", "session_id":session, "stop_hook_active":false, "transcript_path":transcript, "last_assistant_message":reply});
    HookInput::parse(&serde_json::to_vec(&data).unwrap()).unwrap()
}

#[test]
fn a_cause_with_no_quoted_command_log_line_or_measurement_blocks_twice_per_turn_and_a_quoted_one_passes() {
    let dir = std::env::temp_dir().join(format!("north-unproven-cause-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let reply = "The landing stalled because nexus ran out of memory. I restarted the lander.";
    for _ in 0..2 {
        assert!(matches!(decide(&stop(reply, "s1"), &dir), Verdict::Deny(reason) if reason.starts_with("BLOCKED (unproven-cause)") && reason.contains("\"because\"")));
    }
    assert_eq!(decide(&stop(reply, "s1"), &dir), Verdict::Allow);
    assert_eq!(unproven_cause("The cause is a stale runtime.\n\nLanded 4ea48c6.").as_deref(), Some("the cause is"));
    assert_eq!(unproven_cause("It failed, caused by ETXTBSY on exec.").as_deref(), Some("caused by"));
    assert_eq!(unproven_cause("It stalled because `free -m` on nexus printed 112 available."), None);
    assert_eq!(unproven_cause("It stalled because the log says \"DEFER_MEMORY_HEADROOM\"."), None);
    assert_eq!(unproven_cause("It is slow because the test took 45.5 s on ccx33."), None);
    assert_eq!(unproven_cause("Landed the guard on main as 4ea48c6."), None);
}
