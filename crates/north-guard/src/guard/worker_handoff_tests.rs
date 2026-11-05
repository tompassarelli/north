use super::worker_handoff::{Env, decide};
use crate::foundation::clock::Timestamp;
use crate::hook::HookInput;
use serde_json::json;
use std::path::Path;

const NOW: f64 = 1_800_000_000.0;

fn write_transcript(root: &Path, id: &str, tokens: i64, minutes: Option<i64>, filler: usize) {
    let file = root.join("projects/s/subagents").join(format!("agent-{id}.jsonl"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let usage = |input, read, created| json!({"input_tokens": input, "cache_read_input_tokens": read, "cache_creation_input_tokens": created});
    let mut first = json!({"type": "user", "message": {"role": "user", "content": "brief"}});
    if let Some(minutes) = minutes {
        first["timestamp"] = json!(Timestamp::from_unix_seconds_f64(NOW - minutes as f64 * 60.0).iso());
    }
    let lines = [
        first,
        json!({"type": "assistant", "message": {"usage": usage(9, 1000, 0)}}),
        json!({"type": "assistant", "message": {"usage": usage(1000, tokens - 3000, 2000)}}),
        json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "x".repeat(filler)}]}}),
        json!({"type": "attachment"}),
    ];
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(file, text).unwrap();
}

fn payload(root: &Path, id: &str, kind: Option<&str>) -> HookInput {
    let mut data = json!({
        "session_id": "s", "transcript_path": root.join("projects/s.jsonl"), "hook_event_name": "PostToolUse",
        "tool_name": "Bash", "tool_input": {"command": "true"}, "tool_use_id": "toolu_1",
    });
    if let Some(kind) = kind {
        data["agent_id"] = json!(id);
        data["agent_type"] = json!(kind);
    }
    HookInput::parse(data.to_string().as_bytes()).unwrap()
}

fn decide_with(root: &Path, id: &str, kind: Option<&str>, off: bool) -> Option<String> {
    let off = move || off;
    let env = Env { home: root.join("home"), now: NOW, off: &off, confirm: None };
    decide(&payload(root, id, kind), &env)
}

fn ask(root: &Path, id: &str, kind: Option<&str>, tokens: i64, minutes: Option<i64>) -> Option<String> {
    write_transcript(root, id, tokens, minutes, 300_000);
    decide_with(root, id, kind, false)
}

fn handoff_text(root: &Path, id: &str, kilo: i64) -> String {
    let note = root.join("home/.local/state/agents/handoffs").join(format!("{id}.md")).display().to_string();
    format!("Your context is {kilo}k tokens, past the 350k handoff point. Land anything that passes now; if work remains, finish the step you are on, then write a handoff note to {note} with: the brief's goal and Done when list with each box's status; the worktree, branch and commits; running background jobs with their output paths; the next step; evidence paths. Then end your turn with exactly `HANDOFF {note}` and nothing else.")
}

const LEASH: &str = "You have run 45 minutes. Land anything that passes now, then either report (Done:/Not done:/Blocked:) or write a handoff to ~/.local/state/agents/handoffs/<lane>.md, commit your WIP and report Not done: with its path.";

fn context_handoff_repeats_once_per_further_50k() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let high = Some("worker-high");
    assert_eq!(ask(root, "a1", high, 360_000, None), Some(handoff_text(root, "a1", 360)));
    assert_eq!(ask(root, "a1", high, 380_000, None), None);
    assert_eq!(ask(root, "a1", high, 410_000, None), Some(handoff_text(root, "a1", 410)));
    assert_eq!(ask(root, "a1", high, 430_000, None), None);
    assert_eq!(ask(root, "a2", Some("worker"), 349_999, None), None);
    assert_eq!(ask(root, "a3", Some("worker"), 350_000, None), Some(handoff_text(root, "a3", 350)));
    let marker = root.join("home/.local/state/agents/handoffs/.a1.reminded");
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "410000\n");
}

fn only_workers_with_safe_ids_are_told() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    assert_eq!(ask(root, "a4", Some("Explore"), 900_000, None), None);
    assert_eq!(ask(root, "a5", None, 900_000, None), None);
    assert_eq!(ask(root, "a4", Some("Explore"), 1000, Some(90)), None);
    assert!(ask(root, "a6", Some("worker-sonnet"), 900_000, None).is_some());
    assert_eq!(ask(root, &"x".repeat(129), Some("worker"), 900_000, None), None);
    assert!(ask(root, &"y".repeat(128), Some("worker"), 900_000, None).is_some());
}

fn leash_repeats_once_per_further_ten_minutes() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    assert_eq!(ask(root, "t1", Some("worker"), 1000, Some(46)), Some(LEASH.into()));
    assert_eq!(ask(root, "t1", Some("worker"), 1000, Some(50)), None);
    assert_eq!(ask(root, "t1", Some("worker"), 1000, Some(56)), Some(LEASH.into()));
    assert_eq!(ask(root, "t2", Some("worker-haiku"), 1000, Some(44)), None);
    assert_eq!(ask(root, "t3", Some("worker"), 1000, None), None);
    let both = ask(root, "t4", Some("worker"), 360_000, Some(60)).unwrap();
    assert_eq!(both, format!("{} {LEASH}", handoff_text(root, "t4", 360)));
}

fn an_inactive_hook_stays_silent_and_leaves_no_marker() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write_transcript(root, "a7", 900_000, Some(60), 10);
    assert_eq!(decide_with(root, "a7", Some("worker-high"), true), None);
    assert!(!root.join("home").exists());
}

fn the_usage_scan_widens_and_skips_unusable_lines() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let file = root.join("projects/s/subagents/agent-u2.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let usage = json!({"type": "assistant", "message": {"usage": {"input_tokens": 500_000}}});
    let zero = json!({"type": "assistant", "message": {"usage": {"input_tokens": 0}}});
    std::fs::write(&file, format!("{usage}\n{zero}\nnot json \"usage\"\n{{\"type\":\"user\",\"usage\":1}}\n")).unwrap();
    assert!(decide_with(root, "u2", Some("worker"), false).is_some());
    let bad = json!({"type": "assistant", "message": {"usage": {"input_tokens": "many"}}});
    std::fs::write(&file, format!("{usage}\n{bad}\n")).unwrap();
    assert_eq!(decide_with(root, "u2", Some("worker"), false), None);
    assert_eq!(decide_with(root, "missing", Some("worker"), false), None);
}

fn a_window_reads_past_the_first_quarter_megabyte() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write_transcript(root, "w1", 380_000, None, 5_000_000);
    assert_eq!(decide_with(root, "w1", Some("worker"), false), None);
    write_transcript(root, "w2", 380_000, None, 3_000_000);
    assert_eq!(decide_with(root, "w2", Some("worker"), false), Some(handoff_text(root, "w2", 380)));
}

#[test]
fn worker_handoff_decisions_hold() {
    context_handoff_repeats_once_per_further_50k();
    only_workers_with_safe_ids_are_told();
    leash_repeats_once_per_further_ten_minutes();
    an_inactive_hook_stays_silent_and_leaves_no_marker();
    the_usage_scan_widens_and_skips_unusable_lines();
    a_window_reads_past_the_first_quarter_megabyte();
}
