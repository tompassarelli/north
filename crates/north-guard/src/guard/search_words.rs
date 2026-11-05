use crate::shell;
use std::path::Path;

pub(super) const MAX_NESTING: usize = 3;
const WRAPPERS: &[&str] = &[
    "sudo", "doas", "env", "nice", "ionice", "time", "nohup", "command", "builtin", "exec",
    "xargs", "timeout", "stdbuf", "setsid",
];
const WRAPPER_VALUES: &[&str] = &[
    "-u",
    "--user",
    "-g",
    "--group",
    "-n",
    "--adjustment",
    "-c",
    "-e",
    "-I",
    "--replace",
    "-P",
    "--max-procs",
    "-L",
    "--max-lines",
    "-s",
    "--signal",
    "-k",
    "--kill-after",
    "-o",
    "--output",
    "-i",
    "--input",
];
const SHELLS: &[&str] = &["bash", "sh", "dash", "zsh", "ksh"];

pub(super) type Token = (&'static str, String, bool);

pub(super) struct Segment {
    pub words: Vec<String>,
    pub piped: bool,
}

pub(super) enum Located {
    Nested(String),
    Tool(usize),
    Other,
}

pub(super) fn segments(tokens: Vec<Token>) -> Vec<Segment> {
    let mut found = Vec::new();
    let mut words = Vec::new();
    let mut piped = false;
    for (kind, value, _) in tokens {
        if kind == "sep" {
            found.push(Segment {
                words: std::mem::take(&mut words),
                piped,
            });
            piped = value == "|";
        } else {
            words.push(value);
        }
    }
    found.push(Segment { words, piped });
    found
}

fn duration(token: &str) -> bool {
    let digits = token.trim_end_matches(['s', 'm', 'h', 'd']);
    let suffix = &token[digits.len()..];
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && matches!(suffix, "" | "s" | "m" | "h" | "d" | "ms")
}

pub(super) fn locate(
    words: &[String],
    leading: &[&str],
    is_tool: impl Fn(&str) -> bool,
) -> Located {
    let mut index = 0;
    while index < words.len()
        && (shell::assignment(&words[index]) || leading.contains(&words[index].as_str()))
    {
        index += 1;
    }
    while index < words.len() {
        let executable = shell::basename(&words[index]);
        if SHELLS.contains(&executable) {
            return shell_payload(&words[index + 1..]);
        }
        if executable == "eval" {
            return Located::Nested(words[index + 1..].join(" "));
        }
        if is_tool(executable) {
            return Located::Tool(index);
        }
        if !WRAPPERS.contains(&executable) {
            return Located::Other;
        }
        if matches!(executable, "command" | "builtin")
            && words[index + 1..].iter().any(|w| w == "-v" || w == "-V")
        {
            return Located::Other;
        }
        index = skip_wrapper_options(words, index + 1);
    }
    Located::Other
}

fn shell_payload(options: &[String]) -> Located {
    for (position, option) in options.iter().enumerate() {
        if option.starts_with('-') && option[1..].contains('c') {
            return options[position + 1..]
                .iter()
                .find(|word| *word != "--")
                .map_or(Located::Other, |word| Located::Nested(word.clone()));
        }
    }
    Located::Other
}

fn skip_wrapper_options(words: &[String], mut index: usize) -> usize {
    while index < words.len() {
        let token = &words[index];
        if shell::assignment(token) || duration(token) {
            index += 1;
        } else if token == "--" {
            return index + 1;
        } else if token.starts_with('-') {
            let option = token.split('=').next().unwrap_or("");
            index += if !token.contains('=') && WRAPPER_VALUES.contains(&option) {
                2
            } else {
                1
            };
        } else {
            break;
        }
    }
    index
}

pub(super) fn recursive_grep(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "-r" | "-R" | "--recursive" | "--dereference-recursive"
        ) || (argument.len() > 1
            && argument.starts_with('-')
            && argument[1..].bytes().all(|b| b.is_ascii_alphabetic())
            && argument[1..].contains(['r', 'R']))
    })
}

pub(super) fn tilde(word: &str, home: &str) -> String {
    if word == "~" {
        home.to_owned()
    } else if let Some(tail) = word.strip_prefix("~/") {
        format!("{home}/{tail}")
    } else {
        word.to_owned()
    }
}

pub(super) fn expand_home(word: &str, home: &str) -> String {
    tilde(word, home)
        .replace("${HOME}", home)
        .replace("$HOME", home)
}

pub(super) fn join(base: &str, path: &str) -> String {
    Path::new(base).join(path).to_string_lossy().into_owned()
}

pub(super) fn normpath(path: &str) -> String {
    let rooted = path.starts_with('/');
    let leading = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else if rooted {
        "/"
    } else {
        ""
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part != ".." || (!rooted && parts.last().is_none_or(|last| *last == "..")) {
            parts.push(part);
        } else if !parts.is_empty() {
            parts.pop();
        }
    }
    let joined = format!("{leading}{}", parts.join("/"));
    if joined.is_empty() {
        ".".into()
    } else {
        joined
    }
}

pub(super) fn is_file(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

pub(super) fn is_dir(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

pub(super) fn bash_call(input: &crate::hook::HookInput) -> Option<(&str, String)> {
    use crate::platform::pyjson::Value;
    if input.data["tool_name"] != "Bash" {
        return None;
    }
    let command = input.tool_input["command"]
        .as_str()
        .filter(|c| !c.is_empty())?;
    let cwd = match &input.data["cwd"] {
        Value::String(cwd) => match cwd.as_str()? {
            "" => current_dir()?,
            text => text.to_owned(),
        },
        Value::Null | Value::Bool(false) => current_dir()?,
        _ => return None,
    };
    Some((command, cwd))
}

fn current_dir() -> Option<String> {
    Some(std::env::current_dir().ok()?.to_string_lossy().into_owned())
}
