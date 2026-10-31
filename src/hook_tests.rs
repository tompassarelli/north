use super::*;
use serde_json::{Value, json};

fn serde_output(output: Option<pyjson::Value>) -> Option<Value> {
    output.map(|value| serde_json::from_str(&pyjson::dumps(&value)).unwrap())
}

#[test]
fn provider_events_select_matching_guard_list() {
    crate::guard::activation::TEST_GUARDS_OFF.with(|off| off.set(true));
    for (event, tools, explicit, expected) in [
        (
            "PreToolUse",
            vec!["Bash", "exec_command", "functions.exec_command"],
            "pre-bash",
            HookKind::PreBash,
        ),
        (
            "PreToolUse",
            vec![
                "Edit",
                "Write",
                "MultiEdit",
                "NotebookEdit",
                "apply_patch",
                "functions.apply_patch",
            ],
            "pre-edit",
            HookKind::PreEdit,
        ),
        (
            "PreToolUse",
            vec!["Agent", "spawn_agent", "functions.spawn_agent"],
            "pre-agent",
            HookKind::PreAgent,
        ),
        (
            "PostToolUse",
            vec!["Bash", "functions.exec_command"],
            "post-tool",
            HookKind::PostTool,
        ),
        (
            "PermissionRequest",
            vec!["Bash"],
            "permission-request",
            HookKind::PermissionRequest,
        ),
        ("Stop", vec![""], "stop", HookKind::Stop),
        (
            "SubagentStop",
            vec![""],
            "subagent-stop",
            HookKind::SubagentStop,
        ),
        (
            "SessionStart",
            vec![""],
            "session-start",
            HookKind::SessionStart,
        ),
        (
            "UserPromptSubmit",
            vec![""],
            "user-prompt-submit",
            HookKind::UserPromptSubmit,
        ),
    ] {
        assert_eq!(HookKind::explicit(explicit), Some(expected));
        for tool in tools {
            for tool_input in [json!({}), json!("{}")] {
                let bytes = serde_json::to_vec(&json!({"hook_event_name":event, "tool_name":tool, "tool_input":tool_input})).unwrap();
                let input = HookInput::parse(&bytes).unwrap();
                assert_eq!(
                    HookKind::from_input(&input),
                    Some(expected),
                    "{event} {tool}"
                );
                assert_eq!(expected.event(), event);
                let mut output = Vec::new();
                process(bytes.as_slice(), &mut output, &[], "").unwrap();
                assert!(output.is_empty());
            }
        }
    }
    for payload in [
        json!({}),
        json!({"hook_event_name":"unknown"}),
        json!({"hook_event_name":"PreToolUse", "tool_name":"Read"}),
    ] {
        let bytes = serde_json::to_vec(&payload).unwrap();
        assert_eq!(
            HookKind::from_input(&HookInput::parse(&bytes).unwrap()),
            None
        );
        let mut output = Vec::new();
        process(
            bytes.as_slice(),
            &mut output,
            &[|_| panic!("unknown event reached guard")],
            "",
        )
        .unwrap();
        assert!(output.is_empty());
    }
}

fn deny(input: &HookInput) -> Verdict {
    assert_eq!(
        input.command().and_then(|text| text.as_str()),
        Some("echo ready")
    );
    assert_eq!(input.cwd(), Some("/tmp"));
    Verdict::Deny("fixture denial\nverbatim \"reason\"".into())
}

#[test]
fn fixture_denial_preserves_both_provider_inputs_and_first_message() {
    for payload in [
        json!({"tool_name":"Bash", "cwd":"/tmp", "tool_input":{"command":"echo ready"}}),
        json!({"tool_name":"functions.exec_command", "cwd":"/other", "tool_input":"{\"cmd\":\"echo ready\",\"workdir\":\"/tmp\"}"}),
    ] {
        let mut out = Vec::new();
        process(
            serde_json::to_vec(&payload).unwrap().as_slice(),
            &mut out,
            &[deny, |_| panic!("ran after deny")],
            "PreToolUse",
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap(),
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse", "permissionDecision":"deny", "permissionDecisionReason":"fixture denial\nverbatim \"reason\""}})
        );
    }
    for bytes in [
        br#"{"tool_name":"Bash","cwd":"/tmp","tool_input":{"command":"wtype hi","opaque":"\ud800"}}"#.as_slice(),
        br#"{"tool_name":"functions.exec_command","cwd":"/tmp","tool_input":"{\"cmd\":\"wtype hi\",\"opaque\":\"\\ud800\"}"}"#.as_slice(),
        br#"{"tool_name":"Bash","cwd":"/tmp","tool_input":{"command":"wtype hi\ud800","opaque":"\ud800"}}"#.as_slice(),
    ] {
        let mut output = Vec::new();
        process(bytes, &mut output, &[|input| {
            assert!(input.command().unwrap().codepoints.starts_with(&"wtype hi".chars().map(u32::from).collect::<Vec<_>>()));
            Verdict::Deny("No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.".into())
        }, |_| panic!("ran after deny")], "PreToolUse").unwrap();
        assert_eq!(output, b"{\"hookSpecificOutput\": {\"hookEventName\": \"PreToolUse\", \"permissionDecision\": \"deny\", \"permissionDecisionReason\": \"No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.\"}}\n");
        eprintln!("desktop-action opaque surrogate: fixture deny matches Python's 260-byte denial");
    }
}

#[test]
fn python_rewrite_output_preserves_both_provider_inputs() {
    for bytes in [
        br#"{"tool_name":"Bash","tool_input":{"command":"echo ready","opaque":"\ud800"}}"#.as_slice(),
        br#"{"tool_name":"functions.exec_command","tool_input":"{\"cmd\":\"echo ready\",\"opaque\":\"\\ud800\"}"}"#.as_slice(),
    ] {
        let mut output = Vec::new();
        process(bytes, &mut output, &[|input| {
            let mut updated = input.tool_input.clone();
            let field = if input.tool_name() == "Bash" { "command" } else { "cmd" };
            updated[field] = "wrapped".into();
            Verdict::Rewrite(updated, "fixture context".into())
        }], "PreToolUse").unwrap();
        let field = if bytes.starts_with(br#"{"tool_name":"Bash""#) { "command" } else { "cmd" };
        assert_eq!(String::from_utf8(output).unwrap(), format!("{{\"hookSpecificOutput\": {{\"hookEventName\": \"PreToolUse\", \"permissionDecision\": \"allow\", \"updatedInput\": {{\"{field}\": \"wrapped\", \"opaque\": \"\\ud800\"}}, \"additionalContext\": \"fixture context\"}}}}\n"));
    }
}

#[test]
fn python_character_limit_accepts_multibyte_payloads() {
    for character in ['é', '🦀'] {
        for count in [600_000, INPUT_LIMIT as usize - 8, INPUT_LIMIT as usize - 7] {
            let bytes =
                format!("{{\"x\":\"{}\"}}", character.to_string().repeat(count)).into_bytes();
            let accepted = count + 8 <= INPUT_LIMIT as usize;
            assert_eq!(HookInput::parse(&bytes).is_some(), accepted);
            let mut output = Vec::new();
            process(
                bytes.as_slice(),
                &mut output,
                &[|_| Verdict::Deny("accepted".into())],
                "Stop",
            )
            .unwrap();
            assert_eq!(!output.is_empty(), accepted);
        }
    }
}

#[test]
fn malformed_and_oversized_payloads_fail_open() {
    let mut oversized = b"{}".to_vec();
    oversized.resize(INPUT_LIMIT as usize + 1, b' ');
    for bytes in [
        b"{bad".to_vec(),
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{\"tool_input\":\"bad\"}".to_vec(),
        oversized,
    ] {
        let mut output = Vec::new();
        process(
            bytes.as_slice(),
            &mut output,
            &[|_| panic!("invalid payload reached guard")],
            "PreToolUse",
        )
        .unwrap();
        assert!(output.is_empty());
    }
    let mut bytes = b"{}".to_vec();
    bytes.resize(INPUT_LIMIT as usize, b' ');
    let mut output = Vec::new();
    process(
        bytes.as_slice(),
        &mut output,
        &[|_| Verdict::Deny("at limit".into())],
        "Stop",
    )
    .unwrap();
    assert!(!output.is_empty());
}

#[test]
fn stop_denial_blocks_verbatim_and_no_guards_say_nothing() {
    let input = HookInput::parse(b"{}").unwrap();
    assert_eq!(serde_output(evaluate(&input, &[], "Stop")), None);
    assert_eq!(
        serde_output(evaluate(
            &input,
            &[|_| Verdict::Deny("stop\nverbatim".into())],
            "Stop"
        )),
        Some(json!({"decision":"block", "reason":"stop\nverbatim"}))
    );
}

#[test]
fn subagent_stop_denial_blocks_at_top_level() {
    let input = HookInput::parse(b"{}").unwrap();
    assert_eq!(
        serde_output(evaluate(
            &input,
            &[|_| Verdict::Deny("still running".into())],
            "SubagentStop"
        )),
        Some(json!({"decision":"block", "reason":"still running"}))
    );
}

