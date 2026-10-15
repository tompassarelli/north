use crate::hook::HookInput;
use crate::shell::{strip_heredocs, strip_quotes};
use regex::Regex;
use std::sync::LazyLock;

const ARGUMENT_END: [char; 7] = ['\n', ';', '&', '|', ')', '}', '`'];

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_]").unwrap());

pub fn invocation(subcommand: &str, directory_options: bool) -> Regex {
    let space = r"[\s\x1c-\x1f]";
    let directory = if directory_options {
        format!(r"(?:{space}+-C{space}+[^\s\x1c-\x1f]+)*")
    } else {
        String::new()
    };
    Regex::new(&format!(
        r"(?:^|[\n;&|({{`]){space}*(?:sudo{space}+|doas{space}+)?git{directory}{space}+{subcommand}"
    ))
    .unwrap()
}

pub fn bash_command(input: &HookInput) -> Option<String> {
    if !input.raw.windows(3).any(|bytes| bytes == b"git")
        || !input.is_shell()
    {
        return None;
    }
    let command = input.command()?.lossy();
    (!command.is_empty()).then(|| strip_quotes(&strip_heredocs(&command)))
}

pub fn argument_spans<'a>(text: &'a str, invocation: &Regex) -> Vec<&'a str> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    while let Some(hit) = invocation.find_at(text, cursor) {
        cursor = hit.end();
        let tail = &text[cursor..];
        let arguments = &tail[..tail.find(ARGUMENT_END).unwrap_or(tail.len())];
        if WORD.is_match(arguments) {
            continue;
        }
        cursor += arguments.len();
        spans.push(arguments);
    }
    spans
}

pub fn words(arguments: &str) -> impl Iterator<Item = &str> {
    arguments
        .split(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
        .filter(|word| !word.is_empty())
}

pub fn short_cluster(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-') && word[1..].chars().all(|c| c.is_ascii_alphabetic())
}
