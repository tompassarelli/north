//! Which of the programs a launcher owns a command starts itself (north#1020): the caller names them, and this reads
//! every command position, a shell's `-c` payload included, while a maintenance word that only reads a version or the
//! models still runs.

use std::sync::LazyLock;

use regex::Regex;

use super::heredoc::strip_heredocs;
use super::segments::blank_quotes;

const WRAPPERS: &str = r"(?:(?:sudo|doas|env|command|exec|nohup|setsid|time|nice|xargs|then|do|else|elif|if|!|timeout\s+\S+)\s+|[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";
const MAINTENANCE: [&str; 5] = ["--version", "--help", "-h", "-v", "models"];

fn command_position(prefix: &str) -> Regex {
    Regex::new(&format!(r#"{prefix}\s*{WRAPPERS}(?:\S*/)?(?P<tool>[^\s;&|()`'"]+)(?:\s|$)"#)).unwrap()
}

static COMMAND_POSITION: LazyLock<Regex> = LazyLock::new(|| command_position(r"(?:^|[\n;&|(`]|\$\(|\{\s)"));
static SHELL_PAYLOAD: LazyLock<Regex> = LazyLock::new(|| command_position(r#"\b(?:ba|z|da)?sh\s+-\w*c\s+["']"#));

fn found(text: &str, pattern: &Regex, programs: &[&str]) -> Option<String> {
    pattern.captures_iter(text).find_map(|hit| {
        let tool = hit.name("tool")?.as_str();
        let tail = text[hit.get(0)?.end()..].split_whitespace().next();
        (programs.contains(&tool) && !tail.is_some_and(|word| MAINTENANCE.contains(&word))).then(|| tool.to_owned())
    })
}

/// The program among `programs` that `command` starts in command position, or `None` when it starts one of their
/// maintenance words or none of them.
pub fn started(command: &str, programs: &[&str]) -> Option<String> {
    let text = strip_heredocs(command);
    found(&blank_quotes(&text), &COMMAND_POSITION, programs).or_else(|| found(&text, &SHELL_PAYLOAD, programs))
}
