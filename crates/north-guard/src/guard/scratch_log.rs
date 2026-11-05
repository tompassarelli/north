use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::sync::LazyLock;

static GENERIC_LOG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:>>?|\btee(?:\s+-a)?)\s*["']?((?:\$\{?(?:S|SCRATCH|SCRATCHPAD)\}?|\S*/scratchpad)/(?:out|output|build|test|tests|run|check|log)\.log)\b"#).unwrap()
});

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some(command) = input.command().and_then(|command| command.as_str()) else {
        return Verdict::Allow;
    };
    match GENERIC_LOG.captures(command) {
        Some(found) => Verdict::Advise(format!(
            "{} is a generic log name directly under the scratchpad that other workers share, so a parallel worker's run overwrites it; write `$S/<lane>-<name>.log` with your lane in the name.",
            &found[1]
        )),
        None => Verdict::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(command: &str) -> Verdict {
        let payload = serde_json::json!({"tool_name": "Bash", "tool_input": {"command": command, "run_in_background": true}});
        evaluate(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
    }

    #[test]
    fn a_fixed_generic_log_name_under_the_shared_scratchpad_is_warned_and_a_lane_name_passes() {
        for command in ["cargo test > $S/test.log", "cargo test > $S/out.log 2>&1", "cargo build 2>&1 | tee ${S}/build.log", "make >> /tmp/claude-1000/p/s/scratchpad/build.log"] {
            assert!(matches!(verdict(command), Verdict::Advise(text) if text.contains("<lane>")), "{command}");
        }
        for command in ["cargo test > $S/<lane>-test.log", "cargo test > $S/n523-test.log 2>&1", "cargo test > $S/sub/test.log", "cargo test > /tmp/mine/test.log", "cargo test"] {
            assert!(matches!(verdict(command), Verdict::Allow), "{command}");
        }
    }
}
