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

#[test]
fn a_lane_only_command_outside_its_sessions_scope_is_refused_as_its_own_scope_is_not() {
    let outside = "0::/user.slice/user-1000.slice/user@1000.service/north-worker-outside-1.service\n";
    let inside = "0::/user.slice/user-1000.slice/user@1000.service/north-session-lane-77c4.scope\n";
    assert!(lane_only_command_outside_scope(true, Some(outside)), "the broker's fresh --lane child cannot land the session's lane");
    assert!(!lane_only_command_outside_scope(true, Some(inside)), "a command in the session's own scope reaches the broker grant");
    assert!(!lane_only_command_outside_scope(false, Some(outside)), "only a lane-only command takes the lane-only rule");
    assert!(!lane_only_command_outside_scope(true, None), "an unreadable cgroup counts as lane-only, so the broker decides");
}
