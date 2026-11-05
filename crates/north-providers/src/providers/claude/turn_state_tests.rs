use serde_json::json;

use super::*;
use crate::fleet::turns::{Tokens, TurnExit, TurnTotals};

#[test]
fn command_shaped_message_text_reaches_claude_verbatim() {
    let text = "please run north gate open --session x --question q";
    let frame = Claude.message(&Transcript::new(), "u1", text).unwrap();
    let frame: Value = serde_json::from_str(&frame[0]).unwrap();
    assert_eq!(frame["message"]["content"].as_str(), Some(text));
}

#[test]
fn frames_after_a_result_mean_the_session_works_again() {
    let mut transcript = Transcript::new();
    let finished = [
        json!({"type": "user", "isReplay": true, "session_id": "s", "uuid": "u1", "message": {"content": "go"}}),
        json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false}),
        json!({"type": "command_lifecycle", "state": "completed", "session_id": "s"}),
    ];
    for frame in &finished {
        Claude.observe(frame, &mut transcript);
    }
    assert_eq!(transcript.status, Status::Idle);
    let resumed = json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"content": [{"type": "text", "text": "the task reported back"}]}});
    Claude.observe(&resumed, &mut transcript);
    assert_eq!(transcript.status, Status::Working);
}

#[test]
fn revive_hook_frames_leave_an_idle_session_idle() {
    let mut transcript = Transcript::new();
    for frame in [
        json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false}),
        json!({"type": "command_lifecycle", "state": "started", "session_id": "s"}),
        json!({"type": "rate_limit_event", "session_id": "s"}),
    ] {
        Claude.observe(&frame, &mut transcript);
    }
    assert_eq!(transcript.status, Status::Idle);
}

#[test]
fn a_new_claude_user_message_opens_a_turn_before_assistant_output() {
    let user = json!({"type": "user", "session_id": "s", "uuid": "u", "message": {"content": "continue the task"}});
    let mut transcript = Transcript::new();
    Claude.observe(&user, &mut transcript);
    assert_eq!(transcript.status, Status::Working);
    assert!(matches!(Claude.signals(&user).as_slice(), [TurnSignal::Started { .. }, TurnSignal::Prompt { .. }]));
}

#[test]
fn results_carry_turn_usage_and_tasks_announce_themselves() {
    let result = json!({
        "type": "result", "session_id": "s", "is_error": false, "num_turns": 3, "duration_ms": 1200,
        "usage": {"input_tokens": 7, "output_tokens": 40, "cache_read_input_tokens": 900, "cache_creation_input_tokens": 300}
    });
    let tokens = Tokens { input: 7, output: 40, cache_read: 900, cache_write: 300 };
    let totals = TurnTotals { tokens: Some(tokens), turns: Some(3), wall_ms: Some(1200) };
    assert_eq!(Claude.signals(&result), [TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals }]);
    let started = json!({"type": "system", "subtype": "task_started", "session_id": "s", "task_id": "a1"});
    let ended = json!({"type": "system", "subtype": "task_notification", "session_id": "s", "task_id": "a1", "status": "completed"});
    let subagent = json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": "toolu_1", "message": {"content": []}});
    assert_eq!(Claude.signals(&started), [TurnSignal::TaskStarted { conversation: "s".into(), task: "a1".into() }]);
    assert_eq!(Claude.signals(&ended), [TurnSignal::TaskEnded { conversation: "s".into(), task: "a1".into() }]);
    assert_eq!(Claude.signals(&subagent), []);
}

#[test]
fn an_assistant_frame_reports_its_call_usage_while_the_turn_runs() {
    let frame = json!({
        "type": "assistant", "session_id": "s", "parent_tool_use_id": null,
        "message": {"id": "msg_1", "content": [], "usage": {"input_tokens": 5, "output_tokens": 9, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 2}}
    });
    let tokens = Tokens { input: 5, output: 9, cache_read: 100, cache_write: 2 };
    assert_eq!(Claude.signals(&frame), [TurnSignal::Started { conversation: "s".into() }, TurnSignal::Call { conversation: "s".into(), call: "msg_1".into(), tokens }]);
}

#[test]
fn a_compaction_boundary_resets_the_context_to_its_summary_so_a_rollover_runs_once() {
    let frame = json!({"type": "system", "subtype": "compact_boundary", "session_id": "s", "compact_metadata": {"trigger": "manual", "pre_tokens": 23969, "post_tokens": 5669}});
    assert_eq!(Claude.signals(&frame), [TurnSignal::Compacted { conversation: "s".into(), context: 5669 }]);
    let line = Claude.compact().unwrap();
    assert!(line.contains(r#""content":"/compact""#), "a bare /compact user frame, which headless Claude Code runs as the command: {line}");
}
