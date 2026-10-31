use super::*;

#[test]
fn a_tier_naming_a_claude_model_is_gated_as_claude_and_a_codex_tier_is_not() {
    for tier in ["worker", "worker-sonnet", "claude:opus-medium", "sonnet-high", "opus", "opus-xhigh", "haiku", "fable"] {
        assert!(claude_tier(tier), "{tier}");
    }
    for tier in ["codex:medium", "sol-high", "luna-low", "astra-xhigh", ""] {
        assert!(!claude_tier(tier), "{tier}");
    }
}
