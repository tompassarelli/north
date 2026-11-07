use super::*;
use serde_json::json;

const NOTICE: &str = "<task-notification>\n<task-id>bsjri9f9f</task-id>\n<status>completed</status>\n<summary>Background command \"cargo test\" completed (exit code 0)</summary>\n</task-notification>";

fn queued(prompt: &str) -> Value {
    json!({"type": "attachment", "attachment": {"type": "queued_command", "prompt": prompt}})
}

fn injection() -> Value {
    json!({"type": "attachment", "attachment": {"type": "hook_additional_context", "content": ["Grounding (auto-retrieved for this message; terms: audit)\n- crates/north-ops/src/ops/context_audit/scan.rs"]}})
}

fn typed(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": text}})
}

fn audit(lines: &[Value]) -> Grounding {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, lines.iter().map(|line| format!("{line}\n")).collect::<String>()).unwrap();
    audit_file(&path, &path, 0).unwrap().grounding
}

// north#717: a prompt hook's record is written before its prompt, so the record before an injection says nothing about the prompt it grounds.
#[test]
fn counts_an_injection_by_the_prompt_it_grounds_not_the_record_before_it() {
    let after_notice = audit(&[queued(NOTICE), injection(), typed("why did the audit fail")]);
    assert_eq!((after_notice.injections, after_notice.notification_injections), (1, 0));
    let on_notice = audit(&[typed("run it"), injection(), queued(NOTICE)]);
    assert_eq!((on_notice.injections, on_notice.notification_injections), (1, 1));
}
