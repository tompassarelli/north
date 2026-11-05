use crate::hook::{HookInput, Verdict};
use crate::shell::{blank_quotes, strip_heredocs};
use regex::Regex;
use std::sync::LazyLock;

const WRAPPERS: &str = r"(?:(?:sudo|doas|env|command|exec|nohup|setsid|time|nice|xargs|then|do|else|elif|if|!|timeout\s+\S+)\s+|[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";
const FLAGS: &str = r"(?:[ \t]+-\S+(?:[ \t]+[^-\s]\S*)?)*";
const MAINTENANCE: [&str; 5] = ["--version", "--help", "-h", "-v", "models"];

fn reviewer(prefix: &str) -> Regex {
    Regex::new(&format!(
        r"{prefix}\s*{WRAPPERS}(?:\S*/)?(?P<tool>agy|antigravity|gemini|codex{FLAGS}[ \t]+(?:exec{FLAGS}[ \t]+)?review)(?:\s|$)",
    ))
    .unwrap()
}

static COMMAND_POSITION: LazyLock<Regex> =
    LazyLock::new(|| reviewer(r"(?:^|[\n;&|(`]|\$\(|\{\s)"));
static SHELL_PAYLOAD: LazyLock<Regex> =
    LazyLock::new(|| reviewer(r#"\b(?:ba|z|da)?sh\s+-\w*c\s+["']"#));

fn refuses(text: &str, pattern: &Regex) -> bool {
    pattern.captures_iter(text).any(|hit| {
        let tool = &hit["tool"];
        let tail = text[hit.get(0).unwrap().end()..].split_whitespace().next();
        tool.starts_with("codex") || !tail.is_some_and(|word| MAINTENANCE.contains(&word))
    })
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide(input)
}

pub(super) fn decide(input: &HookInput) -> Verdict {
    let Some(command) = input.command().map(|command| command.lossy()) else {
        return Verdict::Allow;
    };
    let text = strip_heredocs(&command);
    if !refuses(&blank_quotes(&text), &COMMAND_POSITION) && !refuses(&text, &SHELL_PAYLOAD) {
        return Verdict::Allow;
    }
    Verdict::Deny(
        "No. Tom's standing order is Claude only: no Gemini, Codex or other non-Claude model as a reviewer. Get the review from a fresh Claude reviewer of a different tier (Opus for a Sonnet author, or Sonnet for an Opus author) by spawning one.".into(),
    )
}

