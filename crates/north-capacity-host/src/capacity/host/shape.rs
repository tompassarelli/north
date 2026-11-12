use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::foundation::offload::{SUBJECT_DIRECTORY, SUBJECT_FILE};

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
static CODE_PATHS: LazyLock<Regex> = LazyLock::new(|| pattern(r"--code[ =]\S+"));

fn redact_word(word: &str) -> String {
    let word = USERINFO.replace_all(word, "${1}<redacted>@");
    let word = ASSIGNMENT.replace_all(&word, "${1}=<redacted>");
    let word = FLAG_VALUE.replace_all(&word, "${1} <redacted>");
    LONG_TOKEN.replace_all(&word, "<redacted>").into_owned()
}

/// The kind of check a recorded command runs: the command with each `--code` path list dropped, since that list names one lane's own changed paths and the same check in another lane is the same kind.
pub fn check_kind(recorded: &[String]) -> Vec<String> {
    let mut kind: Vec<String> = Vec::with_capacity(recorded.len());
    for word in recorded {
        if kind.last().is_none_or(|last| last != "--code") {
            kind.push(CODE_PATHS.replace_all(word, "--code").into_owned());
        }
    }
    kind
}

/// The command as the usage log records it: secrets after secret-named flags and in assignments, URLs and long tokens removed.
pub fn recorded_command(command: &[String]) -> Vec<String> {
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
pub fn command_shape(command: &[String]) -> String {
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

/// Whether `path` is the command file the offload script writes: `SUBJECT_FILE` directly in a directory whose name is `SUBJECT_DIRECTORY` and a nonce, which that script makes for the one file.
fn offload_script(path: &Path) -> bool {
    let made = |name: &str| name.len() > SUBJECT_DIRECTORY.len() && name.starts_with(SUBJECT_DIRECTORY);
    path.is_absolute() && path.file_name().is_some_and(|name| name == SUBJECT_FILE) && path.parent().and_then(Path::file_name).and_then(|name| name.to_str()).is_some_and(made)
}

/// The check a landing wrapper runs from a script file, as `shell -c TEXT`, when the wrapper named that file as the subject it runs and the file is the offload script's own command file; any other command has no subject, and no other file is handed to `read`.
pub fn script_subject(command: &[String], script: Option<&str>, read: impl FnOnce(&Path) -> Option<String>) -> Option<Vec<String>> {
    let script = script.filter(|script| !script.is_empty() && command.last().map(String::as_str) == Some(*script))?;
    let shell = command.first().filter(|first| SHELL_COMMANDS.contains(&program(first)))?;
    let text = Some(Path::new(script)).filter(|path| offload_script(path)).and_then(read)?;
    Some(vec![shell.clone(), "-c".to_owned(), text.trim_end_matches('\n').to_owned()])
}
