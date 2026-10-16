use super::*;

#[test]
fn later_guards_preserve_and_inspect_prior_command_rewrites() {
    for bytes in [
        br#"{"tool_name":"Bash","tool_input":{"command":"original","timeout":1000}}"#.as_slice(),
        br#"{"tool_name":"functions.exec_command","tool_input":"{\"cmd\":\"original\",\"timeout\":1000}"}"#.as_slice(),
    ] {
        let input = HookInput::parse(bytes).unwrap();
        let wrap: Guard = |input| {
            let mut updated = input.tool_input.clone();
            let field = if input.tool_name() == "Bash" { "command" } else { "cmd" };
            updated[field] = format!("wrapped({})", input.command().unwrap().lossy()).into();
            Verdict::Rewrite(updated, "wrapped".into())
        };
        let output = evaluate(&input, &[wrap, wrap], "PreToolUse").unwrap();
        let updated = &output["hookSpecificOutput"]["updatedInput"];
        let field = if input.tool_name() == "Bash" { "command" } else { "cmd" };
        assert_eq!(updated[field].as_str(), Some("wrapped(wrapped(original))"));
        assert_eq!(updated["timeout"], input.tool_input["timeout"]);
        let deny_wrapped: Guard = |input| {
            assert_eq!(input.command().unwrap().lossy(), "wrapped(original)");
            Verdict::Deny("refuse transformed command".into())
        };
        let output = evaluate(&input, &[wrap, deny_wrapped], "PreToolUse").unwrap();
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"].as_str(), Some("deny"));
    }
}
