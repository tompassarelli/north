use crate::hook::{HookInput, Verdict};
use crate::shell::{blank_quotes, strip_heredocs};
use regex::Regex;
use std::sync::LazyLock;

const WRAPPERS: &str = r"(?:(?:sudo|doas|env|command|exec|nohup|setsid|time|nice|xargs|then|do|else|elif|if|!|timeout\s+\S+)\s+|[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";
const FLAGS: &str = r"(?:[ \t]+-\S+(?:[ \t]+[^-\s]\S*)?)*";
const MAINTENANCE: [&str; 5] = ["--version", "--help", "-h", "-v", "models"];

fn reviewer(prefix: &str) -> Regex {
    Regex::new(&format!(
        r"{prefix}\s*{WRAPPERS}(?:\S*/)?(?P<tool>agy|antigravity|gemini|opencode|codex{FLAGS}[ \t]+(?:exec{FLAGS}[ \t]+)?review)(?:\s|$)",
    ))
    .unwrap()
}

static COMMAND_POSITION: LazyLock<Regex> =
    LazyLock::new(|| reviewer(r"(?:^|[\n;&|(`]|\$\(|\{\s)"));
static SHELL_PAYLOAD: LazyLock<Regex> =
    LazyLock::new(|| reviewer(r#"\b(?:ba|z|da)?sh\s+-\w*c\s+["']"#));

fn refuses<'a>(text: &'a str, pattern: &Regex) -> Option<&'a str> {
    pattern.captures_iter(text).find_map(|hit| {
        let tool = hit.name("tool")?.as_str();
        let tail = text[hit.get(0)?.end()..].split_whitespace().next();
        (tool.starts_with("codex") || !tail.is_some_and(|word| MAINTENANCE.contains(&word))).then_some(tool)
    })
}

/// The tool a command runs in command position that no agent may start itself: a non-Claude model as a reviewer, or opencode outside its launcher.
pub(super) fn refused_tool(command: &str) -> Option<String> {
    let text = strip_heredocs(command);
    refuses(&blank_quotes(&text), &COMMAND_POSITION).or_else(|| refuses(&text, &SHELL_PAYLOAD)).map(str::to_owned)
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
    match refused_tool(&command).as_deref() {
        None => Verdict::Allow,
        Some("opencode") => Verdict::Deny(
            "No. opencode runs only through `north fleet launch --provider opencode`, which fixes its model and confines it to one lane; started here it would send repo data to a model nobody routed it to.".into(),
        ),
        Some(_) => Verdict::Deny(
            "No. Tom's standing order is Claude only: no Gemini, Codex or other non-Claude model as a reviewer. Get the review from a fresh Claude reviewer of a different tier (Opus for a Sonnet author, or Sonnet for an Opus author) by spawning one.".into(),
        ),
    }
}

#[cfg(test)]
#[path = "reviewer_cli_tests.rs"]
mod tests;
