use super::*;

#[test]
fn a_deepseek_session_starts_in_a_lane_only_scope_and_a_claude_session_does_not() {
    for provider in ["opencode", "pi"] {
        let unit = session_unit(provider, "77c4");
        assert_eq!(unit, "north-session-lane-77c4.scope");
        assert!(lane_only_cgroup(&format!("0::/user.slice/agent.slice/{unit}\n")), "{provider}");
    }
    for provider in ["claude", "codex"] {
        let unit = session_unit(provider, "77c4");
        assert_eq!(unit, "north-session-77c4.scope");
        assert!(!lane_only_cgroup(&format!("0::/user.slice/agent.slice/{unit}\n")), "{provider}");
    }
    assert!(!lane_only_cgroup("0::/user.slice/north-session-lane-x.service\n"));
    assert_eq!(proved_session(Some("north-author".to_owned()), Some("1")), Some("north-author".to_owned()));
    assert_eq!(proved_session(Some("north-reviewer".to_owned()), None), None, "a forged session name without the broker's proof is not the caller's session");
    assert_eq!(proved_session(Some("north-reviewer".to_owned()), Some("0")), None, "only the proof value 1 counts");
    assert_eq!(proved_session(Some(" ".to_owned()), Some("1")), None, "a blank name is no session");
}
