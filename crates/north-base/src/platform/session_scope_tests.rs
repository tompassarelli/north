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
}
