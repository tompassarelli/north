use crate::fleet::claude_only::DEEPSEEK_PROVIDERS;
use crate::hook_input::{HookInput, Verdict};
use crate::shell::{blank_quotes, strip_heredocs};
use regex::Regex;
use std::sync::LazyLock;

const WRAPPERS: &str = r"(?:(?:sudo|doas|env|command|exec|nohup|setsid|time|nice|xargs|then|do|else|elif|if|!|timeout\s+\S+)\s+|[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";
const FLAGS: &str = r"(?:[ \t]+-\S+(?:[ \t]+[^-\s]\S*)?)*";

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

fn refuses<'a>(text: &'a str, pattern: &Regex) -> Option<&'a str> {
    pattern.captures_iter(text).find_map(|hit| hit.name("tool").map(|tool| tool.as_str()))
}

/// The tool a command runs in command position that no agent may start itself: a non-Claude model as a reviewer, or
/// opencode or pi outside the launcher that owns them.
pub(super) fn refused_tool(command: &str) -> Option<String> {
    if let Some(tool) = crate::shell::started(command, &DEEPSEEK_PROVIDERS) {
        return Some(tool);
    }
    let text = strip_heredocs(command);
    refuses(&blank_quotes(&text), &COMMAND_POSITION).or_else(|| refuses(&text, &SHELL_PAYLOAD)).map(str::to_owned)
}

pub fn decide(input: &HookInput) -> Verdict {
    let Some(command) = input.command() else {
        return Verdict::Allow;
    };
    match refused_tool(command).as_deref() {
        None => Verdict::Allow,
        Some(tool @ ("opencode" | "pi")) => Verdict::Deny(format!(
            "No. {tool} runs only through `north fleet launch --provider {tool}`, which fixes its model and confines it to one lane; started here it would send repo data to a model nobody routed it to."
        )),
        Some(_) => Verdict::Deny(
            "No. Tom's standing order is Claude only: no Gemini, Codex or other non-Claude model as a reviewer. Get the review from a fresh Claude reviewer of a different tier (Opus for a Sonnet author, or Sonnet for an Opus author) by spawning one.".into(),
        ),
    }
}

#[cfg(test)]
#[path = "reviewer_cli_tests.rs"]
mod tests;
