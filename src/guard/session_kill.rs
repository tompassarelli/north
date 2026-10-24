use crate::hook::{HookInput, Verdict};
use crate::shell;
use regex::Regex;
use std::sync::LazyLock;

const TEARDOWN_PREFIXES: &[&str] = &[
    "env", "exec", "nohup", "setsid", "time", "nice", "ionice", "timeout", "stdbuf", "do", "then",
    "else", "!", "{",
];
const LAUNCH_PREFIXES: &[&str] = &["env", "exec", "do", "then", "else", "!", "{"];
const SESSION_PROCESSES: &[&str] = &["niri", "niri-session"];
const SESSION_UNITS: &[&str] = &["niri", "niri.service", "graphical-session.target"];
const LOGINCTL_VERBS: &[&str] = &[
    "terminate-user",
    "kill-user",
    "terminate-session",
    "kill-session",
    "terminate-seat",
];
const PKILL_VALUE_FLAGS: &[&str] = &[
    "-u",
    "-U",
    "--uid",
    "--euid",
    "-g",
    "-G",
    "--group",
    "-s",
    "--signal",
    "-P",
    "--parent",
    "-t",
    "--terminal",
    "-F",
    "--pidfile",
    "-d",
    "--delay",
    "-o",
    "-n",
];
const SSH_VALUE_FLAGS: &str = "BbcDEeFIiJLlmOoPpQRSWw";
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh"];
const HOST_RESTART: &str = "firn host restart-user-manager";

static SIGNAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-(?:[0-9]+|[A-Za-z]+)$").unwrap());
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
    teardown(command, None, 0).or_else(|| detached(command))
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

fn teardown(source: &str, host: Option<&str>, depth: usize) -> Option<Hit> {
    if depth > 8 {
        return None;
    }
    let text = shell::strip_heredocs(source);
    for body in shell::shell_substitutions(&text) {
        if let Some(hit) = teardown(&body, host, depth + 1) {
            return Some(hit);
        }
    }
    segments(&shell::words(&text)?)
        .into_iter()
        .find_map(|(segment, _)| {
            let words = arguments(&segment[shell::command(&segment, TEARDOWN_PREFIXES)..]);
            command_teardown(&words, host, depth)
        })
}

fn command_teardown(words: &[String], host: Option<&str>, depth: usize) -> Option<Hit> {
    let (name, args) = words.split_first()?;
    let shape = match shell::basename(name) {
        "kill" => kill(args),
        "pkill" => pkill(args),
        "killall" => killall(args),
        "loginctl" => loginctl(args),
        "systemctl" => systemctl(args),
        "ssh" => {
            let (destination, remote) = ssh(args)?;
            let remote_host = destination.rsplit('@').next().unwrap_or(&destination);
            return teardown(&remote, Some(remote_host), depth + 1);
        }
        name if SHELLS.contains(&name) => return teardown(shell_script(args)?, host, depth + 1),
        _ => None,
    }?;
    Some(Hit::Teardown {
        shape,
        host: host.map(str::to_owned),
    })
}

fn detached(command: &str) -> Option<Hit> {
    let tokens = shell::words(&shell::strip_heredocs(command))?;
    segments(&tokens)
        .into_iter()
        .find_map(|(segment, background)| launch(&segment, background))
        .map(Hit::Detached)
}

fn segments(tokens: &[String]) -> Vec<(Vec<String>, bool)> {
    let mut out = Vec::new();
    let mut segment: Vec<String> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let redirect = token == "&"
            && (segment.last().is_some_and(|word| word.ends_with('>'))
                || tokens
                    .get(index + 1)
                    .is_some_and(|next| next.starts_with('>')));
        if token == "&" && !redirect {
            out.push((std::mem::take(&mut segment), true));
        } else if shell::is_separator(token) && !redirect {
            out.push((std::mem::take(&mut segment), false));
        } else {
            segment.push(token.clone());
        }
    }
    out.push((segment, false));
    out
}

fn launch(segment: &[String], background: bool) -> Option<String> {
    let words = &segment[shell::command(segment, LAUNCH_PREFIXES)..];
    let (word, args) = match words.split_first() {
        Some((word, args)) => (Some(shell::basename(word)), args),
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

fn kill(args: &[String]) -> Option<&'static str> {
    let mut operands = Vec::new();
    let mut index = 0;
    let mut options = true;
    while index < args.len() {
        let arg = args[index].as_str();
        if options {
            match arg {
                "--" => {
                    options = false;
                    index += 1;
                    continue;
                }
                "-l" | "-L" => return None,
                "-s" | "-n" | "--signal" if index + 1 < args.len() => {
                    index += 2;
                    continue;
                }
                _ if index == 0 && SIGNAL.is_match(arg) && (arg != "-1" || args.len() > 1) => {
                    index += 1;
                    continue;
                }
                _ => {}
            }
        }
        operands.push(arg);
        index += 1;
    }
    operands.contains(&"-1").then_some("broadcast kill -1")
}

fn pkill(args: &[String]) -> Option<&'static str> {
    let mut user = false;
    let mut operands = Vec::new();
    let mut skip = false;
    for arg in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        if matches!(arg.as_str(), "-u" | "-U")
            || arg.starts_with("--uid")
            || arg.starts_with("--euid")
        {
            user = true;
            skip = PKILL_VALUE_FLAGS.contains(&arg.as_str());
            continue;
        }
        if PKILL_VALUE_FLAGS.contains(&arg.as_str()) {
            skip = true;
            continue;
        }
        if !arg.starts_with('-') {
            operands.push(arg.as_str());
        }
    }
    if user && operands.is_empty() {
        Some("user-wide pkill sweep")
    } else if operands.iter().any(|o| SESSION_PROCESSES.contains(o)) {
        Some("compositor kill")
    } else {
        None
    }
}

fn killall(args: &[String]) -> Option<&'static str> {
    if args.iter().any(|a| a == "-u" || a == "--user") {
        Some("user-wide killall sweep")
    } else if args.iter().any(|a| SESSION_PROCESSES.contains(&a.as_str())) {
        Some("compositor kill")
    } else {
        None
    }
}

fn loginctl(args: &[String]) -> Option<&'static str> {
    let verb = args.iter().find(|a| !a.starts_with('-'))?;
    LOGINCTL_VERBS
        .contains(&verb.as_str())
        .then_some("login-session teardown")
}

fn systemctl(args: &[String]) -> Option<&'static str> {
    let user = args.iter().any(|a| a == "--user");
    let mut positional = args.iter().filter(|a| !a.starts_with('-'));
    let verb = positional.next()?.as_str();
    let units: Vec<&str> = positional.map(String::as_str).collect();
    match verb {
        "exit" if user => Some("user-manager exit"),
        "isolate" if units.iter().any(|u| u.starts_with("exit")) => Some("user-manager exit"),
        "stop" | "kill" | "restart" | "try-restart" | "reload-or-restart" => {
            if units.iter().any(|u| u.starts_with("user@")) {
                Some("user-manager teardown")
            } else if user && units.iter().any(|u| SESSION_UNITS.contains(u)) {
                Some("compositor teardown")
            } else {
                None
            }
        }
        _ => None,
    }
}

fn ssh(args: &[String]) -> Option<(String, String)> {
    let mut destination = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if arg == "--" {
            if destination.is_none() {
                destination = args.get(index).cloned();
                index += 1;
            }
            break;
        }
        if let Some(flags) = arg.strip_prefix('-').filter(|flags| !flags.is_empty()) {
            if let Some(at) = flags.find(|c| SSH_VALUE_FLAGS.contains(c))
                && at + 1 == flags.len()
            {
                index += 1;
            }
            continue;
        }
        if destination.is_some() {
            index -= 1;
            break;
        }
        destination = Some(arg.to_owned());
    }
    let remote = args.get(index..).filter(|rest| !rest.is_empty())?.join(" ");
    Some((destination?, remote))
}

fn shell_script(args: &[String]) -> Option<&str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let Some(flags) = arg.strip_prefix('-').filter(|f| !f.starts_with('-')) else {
            return None;
        };
        if flags.contains('c') {
            return iter.next().map(String::as_str);
        }
    }
    None
}

#[cfg(test)]
#[path = "session_kill_tests.rs"]
mod tests;
