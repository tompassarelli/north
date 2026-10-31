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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decide_command(command: &str) -> Verdict {
        let payload = json!({"tool_name":"Bash", "tool_input":{"command":command}});
        decide(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
    }

    #[test]
    fn refuses_non_claude_reviewers_at_command_position() {
        for command in [
            "agy -p review",
            "gemini -p review",
            "/usr/bin/agy --model pro",
            "antigravity chat review",
            "codex exec review this",
            "{ agy -p x; }",
            "if true; then agy -p x; fi",
            "for f in a; do gemini -p x; done",
            "! agy -p x",
            "time gemini -p x",
            "nice agy -p x",
            "xargs agy -p",
            "timeout 30 agy -p x",
            "doas nohup exec agy -p x",
            "echo ok\nagy -p x",
            "echo $(agy -p x)",
            "echo `gemini -p x`",
            "(gemini -p x)",
            "echo hi | gemini -p x",
            "bash -c 'agy -p review'",
            "sh -c \"gemini -p x\"",
            "codex --cd . review",
            "codex exec -m gpt-6 review",
            "codex exec --json review",
            "codex review",
            "CHECK=1 sudo env OTHER=2 agy -p x",
            "printf ready && gemini -p x",
            "CHECK=$(agy -p x); echo $CHECK",
        ] {
            assert!(matches!(decide_command(command), Verdict::Deny(_)), "{command}");
        }
    }

    #[test]
    fn allows_mentions_and_claude_commands() {
        for command in [
            "echo agy",
            "echo 'agy -p review'",
            "rg gemini src",
            "git log --grep=agy",
            "git commit -m 'drop gemini review'",
            "ls codex-notes",
            "codex login",
            "codex exec implement the issue",
            "agy-runtime-update latest",
            "agy --version",
            "agy models",
            "gemini --help",
            "command -v gemini || echo missing",
            "echo \"x; agy -p y\"",
            "bash -c 'echo hi'",
            "codex exec -m gpt-6 implement",
            "echo ${gemini}",
            "claude -p review",
            "cat <<'BODY'\nagy -p x\nBODY",
        ] {
            assert_eq!(decide_command(command), Verdict::Allow, "{command}");
        }
    }
}
