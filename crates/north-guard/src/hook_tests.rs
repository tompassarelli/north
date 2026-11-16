use super::*;
use serde_json::{Value, json};

fn hook(name: &str) -> &'static Hook {
    Hook::named(name).expect("the test names a hook in the dispatch table")
}

#[test]
fn provider_events_select_matching_guard_list() {
    crate::guard::activation::TEST_GUARDS_OFF.with(|off| off.set(true));
    for (event, tools, explicit, expected) in [
        (
            "PreToolUse",
            vec!["Bash", "exec_command", "functions.exec_command"],
            "pre-bash",
            "pre-bash",
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
            "pre-edit",
        ),
        (
            "PreToolUse",
            vec!["Agent", "spawn_agent", "functions.spawn_agent"],
            "pre-agent",
            "pre-agent",
        ),
        (
            "PostToolUse",
            vec!["Bash", "functions.exec_command"],
            "post-tool",
            "post-tool",
        ),
        (
            "PermissionRequest",
            vec!["Bash"],
            "permission-request",
            "permission-request",
        ),
        ("Stop", vec![""], "stop", "stop"),
        (
            "SubagentStop",
            vec![""],
            "subagent-stop",
            "subagent-stop",
        ),
        (
            "SessionStart",
            vec![""],
            "session-start",
            "session-start",
        ),
        (
            "UserPromptSubmit",
            vec![""],
            "user-prompt-submit",
            "user-prompt-submit",
        ),
    ] {
        assert_eq!(Hook::named(explicit).map(|hook| hook.name), Some(expected));
        for tool in tools {
            for tool_input in [json!({}), json!("{}")] {
                let bytes = serde_json::to_vec(&json!({"hook_event_name":event, "tool_name":tool, "tool_input":tool_input})).unwrap();
                let input = HookInput::parse(&bytes).unwrap();
                assert_eq!(
                    Hook::of(&input).map(|hook| hook.name),
                    Some(expected),
                    "{event} {tool}"
                );
                assert_eq!(hook(expected).event(), event);
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
            Hook::of(&HookInput::parse(&bytes).unwrap()).map(|hook| hook.name),
            None
        );
        let mut output = Vec::new();
        process(
            bytes.as_slice(),
            &mut output,
            &[guard("", |_| panic!("unknown event reached guard"))],
            "",
        )
        .unwrap();
        assert!(output.is_empty());
    }
    assert_eq!(Hook::named("pre-read").map(|hook| hook.name), Some("pre-read"));
    assert!(hook("pre-read").guards()[0].0.is_empty(), "no unit name switches the reading guard off");
    let reached = |tool: &str, tool_input: Value| crate::guard::file_read_reach::reached(&HookInput::parse(json!({"hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": tool_input}).to_string().as_bytes()).unwrap());
    assert_eq!(reached("Read", json!({"file_path": "/a/b"})), ["/a/b"]);
    assert_eq!(reached("NotebookRead", json!({"notebook_path": "/a/n.ipynb"})), ["/a/n.ipynb"]);
    assert_eq!(reached("Grep", json!({"pattern": "x", "path": "/a"})), ["/a"]);
    assert_eq!(reached("Grep", json!({"pattern": "x"})), ["."]);
    assert_eq!(reached("Glob", json!({"pattern": "**/*.rs", "path": "/a"})), ["/a/"]);
    assert_eq!(reached("Glob", json!({"pattern": "/h/.ssh/*"})), ["/h/.ssh/"]);
    assert_eq!(reached("Glob", json!({"pattern": "src/x*/y", "path": "/a"})), ["/a/src/"]);
    assert!(reached("Bash", json!({"command": "ls"})).is_empty());
    let (start, prompt) = (hook("session-start").guards(), hook("user-prompt-submit").guards());
    assert!(spare_guards(start, "SessionStart", north_warm_spares::names::Binding::Awaited, || true).is_empty(), "a spare no launch has taken gives no session-start context, which would be the spare's");
    assert_eq!(spare_guards(prompt, "UserPromptSubmit", north_warm_spares::names::Binding::Written, || true).len(), start.len() + prompt.len(), "a taken spare's first prompt gets the session-start context of the session it was bound to");
    assert_eq!(spare_guards(prompt, "UserPromptSubmit", north_warm_spares::names::Binding::Written, || false).len(), prompt.len(), "and only its first");
    assert_eq!(spare_guards(start, "SessionStart", north_warm_spares::names::Binding::None, || true).len(), start.len(), "a session started as no spare keeps its session-start context");
}

fn deny(input: &HookInput) -> Verdict {
    assert_eq!(
        input.command(),
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
            &[guard("", deny), guard("", |_| panic!("ran after deny"))],
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
        process(bytes, &mut output, &[guard("", |input| {
            assert!(input.command().unwrap().starts_with("wtype hi"));
            Verdict::Deny("No. Tom is using the desktop. Use the private desktop or wait until he is idle.".into())
        }), guard("", |_| panic!("ran after deny"))], "PreToolUse").unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&output).unwrap(), json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "No. Tom is using the desktop. Use the private desktop or wait until he is idle."}}));
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
            &[guard("", |_| panic!("invalid payload reached guard"))],
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
        &[guard("", |_| Verdict::Deny("at limit".into()))],
        "Stop",
    )
    .unwrap();
    assert!(!output.is_empty());
}

#[test]
fn stop_denial_blocks_verbatim_and_no_guards_say_nothing() {
    let input = HookInput::parse(b"{}").unwrap();
    assert_eq!(evaluate(&input, &[], "Stop"), None);
    assert_eq!(
        evaluate(
            &input,
            &[guard("", |_| Verdict::Deny("stop\nverbatim".into()))],
            "Stop"
        ),
        Some(json!({"decision":"block", "reason":"stop\nverbatim"}))
    );
}


#[test]
fn an_accepted_finish_call_ends_the_turn_and_a_refused_one_returns_to_the_model() {
    crate::guard::activation::TEST_GUARDS_OFF.with(|off| off.set(true));
    let dispatch = |event: &str, tool: &str, outcome: Value| {
        let mut input = json!({"hook_event_name": event, "tool_name": tool, "tool_input": {"check": "true", "message": "m", "paths": ["/l/a"]}});
        input.as_object_mut().unwrap().extend(outcome.as_object().unwrap().clone());
        let mut output = Vec::new();
        process(serde_json::to_vec(&input).unwrap().as_slice(), &mut output, &[], "").unwrap();
        (!output.is_empty()).then(|| serde_json::from_slice::<Value>(&output).unwrap())
    };
    let accepted = json!({"tool_response": [{"type": "text", "text": "Done: queued /l lane abc, ticket t1"}]});
    let ended = dispatch("PostToolUse", "mcp__north__finish", accepted.clone()).expect("an accepted finish call left the turn running");
    assert_eq!(ended["continue"], false, "{ended}");
    assert!(ended.get("decision").is_none(), "an accepted finish is no refusal: {ended}");
    let refused = json!({"error": "finish: ✗ the check `true` exited 1; NOT committed, NOT queued.", "is_interrupt": false});
    assert_eq!(dispatch("PostToolUseFailure", "mcp__north__finish", refused), None, "a refused finish must reach the model");
    let refused_as_result = json!({"tool_response": [{"type": "text", "text": "north worker-sandbox broker: refused: evalA-lg-settled-outcomes-r1 launched in /tmp/north-assessment/evals/work/lg5/p/worktrees/worker, which is in no project of this host\nfinish: ✗ the check `cargo test` did not start: refused; NOT committed, NOT queued; the attempt is kept in /l/.git/attempt"}]});
    assert_eq!(dispatch("PostToolUse", "mcp__north__finish", refused_as_result), None, "a refused finish Claude Code hands to PostToolUse ended the turn with nothing queued and no refusal reported");
    assert!(dispatch("PostToolUse", "mcp__north__hand-in", accepted).is_none_or(|output| output["continue"] != false), "only the finish call ends the turn");
}

#[test]
fn the_read_guard_refuses_on_a_deadline_miss() {
    let request = json!({"hook_event_name": "PreToolUse", "tool_name": "Read", "tool_input": {"file_path": "/a/b"}}).to_string();
    let answer = deadline_answer(&["pre-read".into()], request.as_bytes()).expect("a read guard that missed its deadline answers");
    assert_eq!(answer["hookSpecificOutput"]["permissionDecision"], "deny", "{answer}");
}
