mod commands;
mod lexical;

use crate::hook::{HookInput, Verdict};
use crate::shell;
use commands::{SHELLS, Script, Systemd, kill, killall, pkill, shell_script, ssh};
use regex::Regex;
use std::sync::LazyLock;

const TEARDOWN_PREFIXES: &[&str] = &[
    "env", "exec", "nohup", "setsid", "time", "nice", "ionice", "timeout", "stdbuf", "do", "then",
    "else", "!", "{",
];
const LAUNCH_PREFIXES: &[&str] = &["env", "exec", "do", "then", "else", "!", "{"];
const HOST_RESTART: &str = "firn host restart-user-manager";

static REDIRECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d*(?:<>|>>|<<|>|<|&>)").unwrap());
static TEMPORARY_SCRIPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/tmp/\S*\.(?:js|mjs|cjs|ts)$").unwrap());
static SCRATCHPAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/tmp/claude-\d+/[^/\s]+/[^/\s]+/scratchpad/\S+$").unwrap());

#[derive(Debug, PartialEq)]
pub(super) enum Hit {
    Teardown {
        shape: &'static str,
        host: Option<String>,
    },
    Detached(String),
}

pub fn evaluate(input: &HookInput) -> Verdict {
    match input.command().and_then(|s| s.as_str()).and_then(decide) {
        Some(hit) => Verdict::Deny(reason(&hit)),
        None => Verdict::Allow,
    }
}

pub(super) fn decide(command: &str) -> Option<Hit> {
    teardown(command, None, 0, &[])
        .or_else(|| lexical::teardown(command))
        .or_else(|| detached(command))
}

fn reason(hit: &Hit) -> String {
    match hit {
        Hit::Teardown {
            shape,
            host: Some(host),
        } => format!(
            "BLOCKED: {shape} on {host} over ssh — this stops every unit, farm runner, container and agent session that host's user manager runs. Run `{HOST_RESTART} {host}` instead: it restarts the user manager only after the host is idle (no busy farm runner job, no fleet session placed there, no running containers, no LAN pool units) and otherwise refuses, naming what blocks. Remote `systemctl --user status|list-units|show` stays allowed."
        ),
        Hit::Teardown { shape, host: None } => format!(
            "BLOCKED: session-killing shape refused ({shape}) — this signals the whole desktop session (compositor, user manager, every agent), not just your target. Signal only processes YOU started, by specific PID (`kill <pid>`) or a unique pattern (`pkill -f '<unique-pattern>'`); manage your own units by exact name. To restart a host's user manager, run `{HOST_RESTART} HOST`, which refuses while the host is in use. Session/compositor teardown is the operator's call. Prose that mentions these phrases is unaffected — only a command-position invocation is denied. Rare explicit override: `north config agents off session-kill-guard` (persistent, live), or a session LAUNCHED with AGENT_NO_AUTHORING_HOOKS=1."
        ),
        Hit::Detached(shape) => format!(
            "BLOCKED: {shape} would detach an agent child from its owner. Use `run-bounded <duration> -- <command>`; the wrapper has a 24h maximum, owns a transient cgroup plus PID namespace, reaps every descendant when the owner ends, and gives all background jobs a shared 48G hard ceiling."
        ),
    }
}

fn teardown(source: &str, host: Option<&str>, depth: usize, stdin: &[String]) -> Option<Hit> {
    if depth > 8 {
        return None;
    }
    let text = shell::strip_heredocs(source);
    for body in shell::shell_substitutions(&text) {
        if let Some(hit) = teardown(&body, host, depth + 1, &[]) {
            return Some(hit);
        }
    }
    let mut heredocs = shell::heredoc_bodies(source)
        .into_iter()
        .map(|body| source[body].to_owned());
    let mut previous: Option<(Vec<String>, Vec<String>)> = None;
    for segment in segments(&tokens(&text)) {
        let mut input = redirected_input(&segment.words, &mut heredocs);
        if input.is_empty() {
            input = match (&previous, segment.piped) {
                (Some((words, input)), true) => produced(words, input),
                (_, true) => Vec::new(),
                (_, false) => stdin.to_vec(),
            };
        }
        let words = arguments(&segment.words[shell::command(&segment.words, TEARDOWN_PREFIXES)..]);
        if let Some(hit) = command_teardown(&words, host, depth, &input) {
            return Some(hit);
        }
        previous = Some((words, input));
    }
    None
}

fn command_teardown(
    words: &[String],
    host: Option<&str>,
    depth: usize,
    stdin: &[String],
) -> Option<Hit> {
    let (name, args) = words.split_first()?;
    let shape = match shell::basename(name) {
        "kill" => kill(args),
        "pkill" => pkill(args),
        "killall" => killall(args),
        name @ ("loginctl" | "systemctl") => {
            let call = Systemd::parse(args);
            let shape = if name == "loginctl" {
                call.loginctl()
            } else {
                call.systemctl()
            }?;
            return Some(Hit::Teardown {
                shape,
                host: call.host.or(host.map(str::to_owned)),
            });
        }
        "ssh" => {
            let (destination, remote) = ssh(args)?;
            let remote_host = destination.rsplit('@').next().unwrap_or(&destination);
            return match remote {
                Some(remote) => teardown(&remote, Some(remote_host), depth + 1, stdin),
                None => scripts(stdin, Some(remote_host), depth),
            };
        }
        "eval" => return teardown(&args.join(" "), host, depth + 1, stdin),
        name if SHELLS.contains(&name) => {
            return match shell_script(args) {
                Script::Inline(script) => teardown(script, host, depth + 1, &[]),
                Script::Stdin => scripts(stdin, host, depth),
                Script::File => None,
            };
        }
        _ => None,
    }?;
    Some(Hit::Teardown {
        shape,
        host: host.map(str::to_owned),
    })
}

fn scripts(stdin: &[String], host: Option<&str>, depth: usize) -> Option<Hit> {
    stdin
        .iter()
        .find_map(|script| teardown(script, host, depth + 1, &[]))
}

fn redirected_input(words: &[String], heredocs: &mut impl Iterator<Item = String>) -> Vec<String> {
    let mut input = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let operator = word.trim_start_matches(|c: char| c.is_ascii_digit());
        if let Some(string) = operator.strip_prefix("<<<") {
            input.extend(match string {
                "" => words.get(index + 1).cloned(),
                string => Some(string.to_owned()),
            });
        } else if operator.starts_with("<<") {
            input.extend(heredocs.next());
        }
    }
    input
}

fn produced(words: &[String], stdin: &[String]) -> Vec<String> {
    let Some((name, args)) = words.split_first() else {
        return Vec::new();
    };
    match shell::basename(name) {
        "echo" => {
            let start = args
                .iter()
                .take_while(|arg| matches!(arg.as_str(), "-n" | "-e" | "-E" | "-ne" | "-en"))
                .count();
            vec![args[start..].join(" ")]
        }
        "printf" => args.iter().cloned().chain([args.join(" ")]).collect(),
        "cat" if args.iter().all(|arg| arg.starts_with('-')) => stdin.to_vec(),
        _ => Vec::new(),
    }
}

fn detached(command: &str) -> Option<Hit> {
    segments(&tokens(&shell::strip_heredocs(command)))
        .into_iter()
        .find_map(|segment| {
            let words = &segment.words[shell::command(&segment.words, LAUNCH_PREFIXES)..];
            launch(words, segment.background)
        })
        .or_else(|| lexical::detached(command))
        .map(Hit::Detached)
}

fn tokens(text: &str) -> Vec<String> {
    if let Some(tokens) = shell::words(text) {
        return tokens;
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let (mut tokens, rest) = (0..lines.len())
        .rev()
        .find_map(|complete| {
            shell::words(&lines[..complete].concat())
                .map(|tokens| (tokens, lines[complete..].concat()))
        })
        .unwrap_or_default();
    let unquoted = rest.replace(['\'', '"', '\\'], " ");
    tokens.push(";".into());
    tokens.extend(shell::words(&unquoted).unwrap_or_default());
    tokens
}

struct Segment {
    words: Vec<String>,
    background: bool,
    piped: bool,
}

fn segments(tokens: &[String]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut piped = false;
    for (index, token) in tokens.iter().enumerate() {
        let redirect = token == "&"
            && (words.last().is_some_and(|word| word.ends_with('>'))
                || tokens
                    .get(index + 1)
                    .is_some_and(|next| next.starts_with('>')));
        if shell::is_separator(token) && !redirect {
            let background = token == "&";
            out.push(Segment {
                words: std::mem::take(&mut words),
                background,
                piped,
            });
            piped = matches!(token.trim_end_matches('\n'), "|" | "|&");
        } else {
            words.push(token.clone());
        }
    }
    out.push(Segment {
        words,
        background: false,
        piped,
    });
    out
}

fn launch(words: &[String], background: bool) -> Option<String> {
    let (word, args) = match words.split_first() {
        Some((word, args)) => (Some(shell::basename(word.trim_end_matches('/'))), args),
        None => (None, words),
    };
    match word {
        Some(name @ ("nohup" | "setsid" | "disown")) => Some(name.into()),
        Some(name @ ("bun" | "node")) if args.iter().any(|arg| temporary_script(arg)) => {
            Some(format!("direct {name} /tmp script"))
        }
        Some("run-bounded") => None,
        _ if background => Some("unmanaged background job".into()),
        _ => None,
    }
}

fn temporary_script(arg: &str) -> bool {
    TEMPORARY_SCRIPT.is_match(arg) && !(SCRATCHPAD.is_match(arg) && !arg.contains("/../"))
}

fn arguments(words: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for word in words {
        if std::mem::take(&mut skip) {
            continue;
        }
        if REDIRECTION.is_match(word) {
            skip = word.ends_with(['>', '<']);
            continue;
        }
        out.push(word.clone());
    }
    out
}

#[cfg(test)]
#[path = "session_kill_tests.rs"]
mod tests;
