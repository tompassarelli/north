#![cfg(test)]

use super::status_not_action::{announced_step, decide, nothing_needed};
use crate::hook::{HookInput, Verdict};
use serde_json::json;

fn stop(reply: &str, session: &str) -> HookInput {
    let transcript = format!("{}/src/guard/fixtures/recommend_acts/acted.jsonl", env!("CARGO_MANIFEST_DIR"));
    let data = json!({"hook_event_name":"Stop", "agent_type":"proxy", "session_id":session, "stop_hook_active":false, "transcript_path":transcript, "last_assistant_message":reply});
    HookInput::parse(&serde_json::to_vec(&data).unwrap()).unwrap()
}

#[test]
fn a_final_sentence_announcing_a_step_or_saying_nothing_is_needed_blocks_twice_per_turn_and_then_passes() {
    let dir = std::env::temp_dir().join(format!("north-status-not-action-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let reply = "Landed the guard on main. Next I'll run the audit against yesterday's replies.";
    for _ in 0..2 {
        assert!(matches!(decide(&stop(reply, "s1"), &dir), Verdict::Deny(reason) if reason.starts_with("BLOCKED (status-not-action)") && reason.contains("\"i'll\"") && reason.contains("or on the one decision Tom must make as a multiple-choice question.") && !reason.contains("and what Tom must")));
    }
    assert_eq!(decide(&stop(reply, "s1"), &dir), Verdict::Allow);
    let quiet = "Landed the gate digest on main as 4ea48c6. Nothing needs your decision.";
    assert!(matches!(decide(&stop(quiet, "s2"), &dir), Verdict::Deny(reason) if reason.contains("\"nothing needs your decision\"") && reason.contains("noise")));
}

#[test]
fn only_the_last_sentence_counts_and_a_finished_report_passes() {
    assert_eq!(announced_step("I'll check the lander. It is landed as 4ea48c6."), None);
    assert_eq!(announced_step("Done: the guard is on main and its fixture tests pass."), None);
    assert_eq!(announced_step("The audit is written. I\u{2019}ll file the follow-up now").as_deref(), Some("i'll"));
    assert_eq!(announced_step("Everything is in place and the lander will now rebuild").as_deref(), Some("will now"));
    assert_eq!(nothing_needed("Gate 169 waits on you. No action needed from the fleet").as_deref(), Some("no action needed"));
    assert_eq!(nothing_needed("No action needed on gate 12, so I closed it. Landed 4ea48c6."), None);
    assert_eq!(nothing_needed("Which key should nexus accept: one per host, or none?"), None);
}
