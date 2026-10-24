use super::commands::{Systemd, kill, killall, pkill};
use super::{Hit, REDIRECTION};
use crate::shell;
use regex::Regex;
use std::sync::LazyLock;

const WRAPPERS: &[&str] = &["command", "exec", "env", "sudo", "doas"];
const CONTROL: &[&str] = &[";", "&&", "||", "|", "|&", "&", "\n", "(", ")"];

static COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[\n;&|({`])[\s\x1c-\x1f]*(?:(?:sudo|doas|command|exec|nohup|setsid)[\s\x1c-\x1f]+)*(kill|pkill|killall|loginctl|systemctl)[\s\x1c-\x1f]+([^\n;&|)}`]*)",
    )
    .unwrap()
});
static HEREDOC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<<-?[\s\x1c-\x1f]*(['"]?)([\p{L}\p{N}_]+)"#).unwrap());

pub(super) fn teardown(command: &str) -> Option<Hit> {
    let text = shell::strip_quotes(&strip_heredocs(command));
    COMMAND.captures_iter(&text).find_map(|call| {
        let args: Vec<String> = call[2]
            .split(shell::whitespace)
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect();
        let shape = match &call[1] {
            "kill" => kill(&args),
            "pkill" => pkill(&args),
            "killall" => killall(&args),
            "loginctl" => Systemd::flat(&args).loginctl(),
            _ => Systemd::flat(&args).systemctl(),
        }?;
        Some(Hit::Teardown { shape, host: None })
    })
}

pub(super) fn detached(command: &str) -> Option<String> {
    let tokens = shell::lex(&strip_heredocs(command))?;
    let mut segment: Vec<String> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !CONTROL.contains(&token.as_str()) {
            segment.push(token.clone());
            continue;
        }
        let redirect = segment.last().is_some_and(|word| word.ends_with('>'))
            || tokens
                .get(index + 1)
                .is_some_and(|next| next.starts_with('>'));
        if let Some(hit) = launch(&segment, token == "&" && !redirect) {
            return Some(hit);
        }
        segment.clear();
    }
    launch(&segment, false)
}

fn launch(segment: &[String], background: bool) -> Option<String> {
    let start = segment
        .iter()
        .take_while(|word| shell::assignment(word) || REDIRECTION.is_match(word))
        .count();
    let words = &segment[start..];
    super::launch(&words[shell::command(words, WRAPPERS)..], background)
}

fn strip_heredocs(text: &str) -> String {
    let mut out = String::new();
    let (mut kept, mut search) = (0, 0);
    while let Some(start) = HEREDOC.captures_at(text, search) {
        let whole = start.get(0).unwrap();
        let quote = &start[1];
        if !text[whole.end()..].starts_with(quote) {
            search = whole.start() + 1;
            continue;
        }
        let end = whole.end() + quote.len();
        let Some(body) = text[end..].find('\n').map(|n| end + n + 1) else {
            break;
        };
        let terminator =
            Regex::new(&format!(r"(?m)^[ \t]*{}[ \t]*$", regex::escape(&start[2]))).unwrap();
        out.push_str(&text[kept..body]);
        let close = terminator
            .find_at(text, body)
            .map_or(text.len(), |m| m.start());
        out.push_str(&shell::blank(&text[body..close]));
        kept = close;
        search = close;
    }
    out.push_str(&text[kept..]);
    out
}
