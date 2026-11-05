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

