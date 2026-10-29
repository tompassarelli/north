use std::sync::LazyLock;

use regex::Regex;

const SHELL_COMMANDS: [&str; 4] = ["bash", "sh", "zsh", "dash"];
const SHELL_BUILTINS: [&str; 10] = ["cd", "wait", "echo", "printf", "export", "set", "true", "exit", "source", "."];
const WRAPPER_COMMANDS: [&str; 8] = ["exec", "env", "nice", "ionice", "timeout", "time", "nohup", "stdbuf"];
const SECRET_WORDS: &str = "(?:token|secret|password|passwd|api-?key|auth|credential)";

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("machine-capacity pattern")
}

static SECRET_FLAG: LazyLock<Regex> = LazyLock::new(|| pattern(&format!("(?i)^--?[A-Za-z0-9_-]*{SECRET_WORDS}[A-Za-z0-9_-]*$")));
static USERINFO: LazyLock<Regex> = LazyLock::new(|| pattern(r"(://)[^/\s@]+@"));
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r#"\b([A-Za-z_][A-Za-z0-9_-]*)=("[^"]*"|'[^']*'|\S+)"#));
static FLAG_VALUE: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r#"(?i)(--?[A-Za-z0-9_-]*{SECRET_WORDS}[A-Za-z0-9_-]*)\s+("[^"]*"|'[^']*'|\S+)"#)));
static LONG_TOKEN: LazyLock<Regex> = LazyLock::new(|| pattern(r"[A-Za-z0-9+/_-]{32,}={0,2}"));
static VARIABLE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Za-z_][A-Za-z0-9_]*="));
static DURATION: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[0-9.]+[smhd]?$"));
static SUBCOMMAND: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[a-z][a-z0-9:_-]*$"));

fn redact_word(word: &str) -> String {
    let word = USERINFO.replace_all(word, "${1}<redacted>@");
    let word = ASSIGNMENT.replace_all(&word, "${1}=<redacted>");
    let word = FLAG_VALUE.replace_all(&word, "${1} <redacted>");
    LONG_TOKEN.replace_all(&word, "<redacted>").into_owned()
}

/// The command as the usage log records it: secrets after secret-named flags and in assignments, URLs and long tokens removed.
pub(crate) fn recorded_command(command: &[String]) -> Vec<String> {
    command
        .iter()
        .enumerate()
        .map(|(index, word)| {
            if index > 0 && SECRET_FLAG.is_match(&command[index - 1]) {
                "<redacted>".to_owned()
            } else {
                redact_word(word).chars().take(2000).collect()
            }
        })
        .collect()
}

fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn shell_inner(script: &str) -> Vec<String> {
    script
        .split([';', '&', '|', '\n'])
        .map(|part| part.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .find(|part| part.first().is_some_and(|first| !SHELL_BUILTINS.contains(&first.as_str())))
        .unwrap_or_default()
}

fn unwrap_once(words: &[String], unwrapped_shell: &mut bool) -> Option<Vec<String>> {
    let name = program(&words[0]);
    if !*unwrapped_shell && SHELL_COMMANDS.contains(&name) && words.get(1).map(String::as_str) == Some("-c") && words.len() > 2 {
        *unwrapped_shell = true;
        return Some(shell_inner(&words[2]));
    }
    if name == "nix" && matches!(words.get(1).map(String::as_str), Some("develop" | "shell")) {
        let marker = words.iter().position(|word| word == "-c" || word == "--command")?;
        return Some(words[marker + 1..].to_vec());
    }
    if WRAPPER_COMMANDS.contains(&name) || VARIABLE.is_match(&words[0]) {
        let skip = words[1..].iter().take_while(|word| word.starts_with('-') || VARIABLE.is_match(word) || DURATION.is_match(word)).count();
        return Some(words[1 + skip..].to_vec());
    }
    None
}

/// A stable key for "the same kind of work": the program and its subcommand words.
pub(crate) fn command_shape(command: &[String]) -> String {
    let mut words = command.to_vec();
    let mut unwrapped_shell = false;
    for _ in 0..8 {
        if words.is_empty() {
            break;
        }
        match unwrap_once(&words, &mut unwrapped_shell) {
            Some(inner) => words = inner,
            None => break,
        }
    }
    let Some(first) = words.first() else {
        return "unknown".to_owned();
    };
    let mut shape = vec![program(first).to_owned()];
    shape.extend(words[1..].iter().take_while(|word| SUBCOMMAND.is_match(word)).take(2).cloned());
    shape.join(" ")
}
