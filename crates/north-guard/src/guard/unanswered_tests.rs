#![cfg(test)]

use super::unanswered::{asks, decide};
use crate::hook_input::{HookInput, Verdict};
use serde_json::json;
use std::path::PathBuf;

fn transcript(test: &str, prompt: &str, reply: &str) -> String {
    let dir = std::env::temp_dir().join(format!("north-unanswered-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.jsonl");
    let lines = [
        json!({"type":"user", "uuid":"t1", "message":{"role":"user", "content":prompt}}),
        json!({"type":"assistant", "uuid":"a1", "message":{"role":"assistant", "content":[{"type":"text", "text":reply}]}}),
    ];
    std::fs::write(&path, lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n") + "\n").unwrap();
    path.display().to_string()
}

fn stop(test: &str, prompt: &str, reply: &str) -> HookInput {
    let data = json!({"hook_event_name":"Stop", "agent_type":"coordinator", "session_id":"s1", "stop_hook_active":false, "transcript_path":transcript(test, prompt, reply)});
    HookInput::parse(&serde_json::to_vec(&data).unwrap()).unwrap()
}

fn state(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("north-unanswered-state-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

pub(super) fn unanswered_guard_cases() {
    assert!(
        matches!(decide(&stop("blocks", "Did you land the monitor fix?", "Split: the timer needs the laptop first."), &state("blocks")), Verdict::Deny(_)),
        "a reply opening with status and no answer is refused"
    );
    assert!(
        matches!(decide(&stop("issue-number", "Did you land it?", "Done: north#641 queued on the lander."), &state("issue-number")), Verdict::Deny(_)),
        "an issue number is not the yes or no a yes-or-no question asks for"
    );
    assert_eq!(decide(&stop("yes", "Did you land the monitor fix?", "Yes, the fix is on origin/main."), &state("yes")), Verdict::Allow);
    assert_eq!(decide(&stop("no", "Is the timer still absent on gex45?", "No."), &state("no")), Verdict::Allow);
    assert_eq!(decide(&stop("number", "How many boxes are still open?", "Two boxes are open."), &state("number")), Verdict::Allow);
    assert_eq!(decide(&stop("fact", "Where did the lander move?", "The lander moved to gex45."), &state("fact")), Verdict::Allow);
    assert_eq!(decide(&stop("plain", "Land the monitor fix and report back.", "Done: the fix is on main."), &state("plain")), Verdict::Allow);
    let capped = stop("cap", "Did the lander move?", "Done: queued the change.\n\nYes, it moved.");
    let capped_state = state("cap");
    assert!(matches!(decide(&capped, &capped_state), Verdict::Deny(_)));
    assert!(matches!(decide(&capped, &capped_state), Verdict::Deny(_)));
    assert_eq!(decide(&capped, &capped_state), Verdict::Allow, "a third refusal would loop the turn forever");
    assert!(asks("Did you answer my question"));
    assert!(asks("so why did the lander fail?"));
    assert!(asks("what is left"));
    assert!(!asks("you didn't answer my question"));
    assert!(!asks("Land it now"));
    assert!(!asks("Land the fix; the log said `timeout? retry`."));
}
