mod background;
mod commands;
mod lexical;
mod reason;

use crate::hook_input::{HookInput, Verdict};
use serde_json::Value;
use crate::shell;
use commands::{DYNAMIC_UNIT, SHELLS, Script, Systemd, full_pattern, kill, killall, pkill, shell_script, ssh};
use reason::reason;
use regex::Regex;
use rustix::process::getuid;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;
use std::sync::LazyLock;

const TEARDOWN_PREFIXES: &[&str] = &[
    "env", "exec", "nohup", "setsid", "time", "nice", "ionice", "timeout", "stdbuf", "do", "then",
    "else", "while", "until", "if", "elif", "!", "{",
];
const LAUNCH_PREFIXES: &[&str] = &["env", "exec", "do", "then", "else", "!", "{"];
const HOST_RESTART: &str = "firn host restart-user-manager";
const UNMANAGED: &str = "unmanaged background job";
const NO_BUS_REASON: &str = "BLOCKED: a background `&` or nohup job would detach an agent child from its owner, and `north capacity bounded` cannot start here because this session has no systemd user bus. Run the command as written, without `&` or nohup, and set run_in_background: true on the Bash call; its completion notification replaces the wait.";
const REWRITE_NOTE: &str = "session-kill-guard: wrapped the background `&` job in `north capacity bounded 24h --` so the owner reaps it. Write `north capacity bounded <duration> -- <command> &` yourself next time.";

static REDIRECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d*(?:<>|>>|<<|>|<|&>)").unwrap());

static CONSUMER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[\n;&|({`'"]|\b(?:do|then|else))[ \t]*(?:(?:until|while)[ \t]|(?:kill|xargs|wait)\b)|--pid\b"#).unwrap()
});

struct Context<'a> {
    root: &'a str,
    own: Vec<String>,
    pgrep_consumed: bool,
}

#[derive(Debug, PartialEq)]
pub(super) enum Hit {
    Teardown {
        shape: &'static str,
        host: Option<String>,
    },
    Detached(String),
    SelfMatch {
        tool: &'static str,
        pattern: String,
    },
    CrossSession {
        tool: &'static str,
        operands: String,
    },
}

pub fn evaluate(input: &HookInput) -> Verdict {
    evaluate_with(input, user_bus())
}

fn user_bus() -> bool {
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| PathBuf::from(format!("/run/user/{}", getuid().as_raw())), PathBuf::from);
    fs::metadata(runtime.join("bus")).is_ok_and(|bus| bus.file_type().is_socket())
}

fn evaluate_with(input: &HookInput, bus: bool) -> Verdict {
    let hits = input.command().map(decide).unwrap_or_default();
    if hits.is_empty() {
        return Verdict::Allow;
    }
    if !bus && wrappable(&hits) {
        return Verdict::Deny(NO_BUS_REASON.into());
    }
    managed(input, &hits).map_or_else(
        || Verdict::Deny(reason(&hits)),
        |updated| Verdict::Rewrite(updated, REWRITE_NOTE.into()),
    )
}

fn wrappable(hits: &[Hit]) -> bool {
    hits.iter().all(|hit| matches!(hit, Hit::Detached(shape) if shape == UNMANAGED || shape == "nohup"))
}

fn managed(input: &HookInput, hits: &[Hit]) -> Option<Value> {
    if !wrappable(hits) || input.data["tool_name"] != "Bash" {
        return None;
    }
    let command = input.tool_input["command"].as_str()?;
    let candidate = background::rewrite(command)?;
    if !decide(&candidate).is_empty() {
        return None;
    }
    let mut updated = input.tool_input.clone();
    updated["command"] = candidate.into();
    Some(updated)
}

pub(super) fn decide(command: &str) -> Vec<Hit> {
    decide_in(command, &fs::read_to_string("/proc/self/cgroup").unwrap_or_default())
}

fn decide_in(command: &str, cgroup: &str) -> Vec<Hit> {
    let ctx = Context {
        root: command,
        own: commands::cgroup_units(cgroup),
        pgrep_consumed: CONSUMER.is_match(&shell::strip_heredocs(command)),
    };
    let mut hits = teardown(command, None, 0, &[], &ctx);
    if hits.is_empty() {
        hits.extend(lexical::teardown(command, &ctx.own));
    }
    hits.extend(detached(command));
    let mut unique = Vec::new();
    for hit in hits {
        if !unique.contains(&hit) {
            unique.push(hit);
        }
    }
    unique
}

fn teardown(
    source: &str,
    host: Option<&str>,
    depth: usize,
    stdin: &[String],
    ctx: &Context,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    if depth > 8 {
        return hits;
    }
    let text = shell::strip_heredocs(source);
    for body in shell::shell_substitutions(&text) {
        hits.extend(teardown(&body, host, depth + 1, &[], ctx));
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
        hits.extend(command_teardown(&words, host, depth, &input, ctx));
        previous = Some((words, input));
    }
    hits
}

fn command_teardown(
    words: &[String],
    host: Option<&str>,
    depth: usize,
    stdin: &[String],
    ctx: &Context,
) -> Vec<Hit> {
    let Some((name, args)) = words.split_first() else {
        return Vec::new();
    };
    let shape = match shell::basename(name) {
        "kill" => kill(args),
        "pkill" => return by_name("pkill", pkill(args), args, host, ctx),
        "killall" => return by_name("killall", killall(args), args, host, ctx),
        "pgrep" => return self_match("pgrep", args, ctx).filter(|_| ctx.pgrep_consumed).into_iter().collect(),
        name @ ("loginctl" | "systemctl") => {
            let call = Systemd::parse(args);
            let local = host.is_none() && call.host.is_none();
            let shape = if name == "loginctl" {
                call.loginctl()
            } else {
                call.systemctl(local.then_some(ctx.own.as_slice()))
            };
            return shape
                .map(|shape| Hit::Teardown {
                    shape,
                    host: call.host.or(host.map(str::to_owned)),
                })
                .into_iter()
                .collect();
        }
        "ssh" => {
            let Some((destination, remote)) = ssh(args) else {
                return Vec::new();
            };
            let remote_host = destination.rsplit('@').next().unwrap_or(&destination);
            return match remote {
                Some(remote) => teardown(&remote, Some(remote_host), depth + 1, stdin, ctx),
                None => scripts(stdin, Some(remote_host), depth, ctx),
            };
        }
        "xargs" => return fed_units(args, host, depth, stdin, ctx),
        "eval" => return teardown(&args.join(" "), host, depth + 1, stdin, ctx),
        name if SHELLS.contains(&name) => {
            return match shell_script(args) {
                Script::Inline(script) => teardown(script, host, depth + 1, &[], ctx),
                Script::Stdin => scripts(stdin, host, depth, ctx),
                Script::File => Vec::new(),
            };
        }
        _ => None,
    };
    shape
        .map(|shape| Hit::Teardown {
            shape,
            host: host.map(str::to_owned),
        })
        .into_iter()
        .collect()
}

fn fed_units(
    args: &[String],
    host: Option<&str>,
    depth: usize,
    stdin: &[String],
    ctx: &Context,
) -> Vec<Hit> {
    let Some(at) = args
        .iter()
        .position(|arg| shell::basename(arg) == "systemctl" || SHELLS.contains(&shell::basename(arg)))
    else {
        return Vec::new();
    };
    if shell::basename(&args[at]) != "systemctl" {
        return command_teardown(&args[at..], host, depth + 1, stdin, ctx);
    }
    let call = Systemd::parse(&args[at + 1..]);
    (host.is_none() && call.host.is_none() && call.teardown_verb())
        .then_some(Hit::Teardown {
            shape: DYNAMIC_UNIT,
            host: None,
        })
        .into_iter()
        .collect()
}

fn self_match(tool: &'static str, args: &[String], ctx: &Context) -> Option<Hit> {
    let (pattern, insensitive) = full_pattern(args)?;
    let source = if insensitive {
        format!("(?i){pattern}")
    } else {
        pattern.clone()
    };
    let matched =
        Regex::new(&source).map_or_else(|_| ctx.root.contains(&pattern), |re| re.is_match(ctx.root));
    matched.then_some(Hit::SelfMatch { tool, pattern })
}

fn by_name(tool: &'static str, shape: Option<&'static str>, args: &[String], host: Option<&str>, ctx: &Context) -> Vec<Hit> {
    let hit = match shape {
        Some(shape) => Some(Hit::Teardown { shape, host: host.map(str::to_owned) }),
        None if commands::cross_session(tool, args) => Some(Hit::CrossSession { tool, operands: args.join(" ") }),
        None if tool == "pkill" => self_match(tool, args, ctx),
        None => None,
    };
    hit.into_iter().collect()
}

fn scripts(stdin: &[String], host: Option<&str>, depth: usize, ctx: &Context) -> Vec<Hit> {
    stdin
        .iter()
        .flat_map(|script| teardown(script, host, depth + 1, &[], ctx))
        .collect()
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

fn detached(command: &str) -> Vec<Hit> {
    let parsed = segments(&tokens(&shell::strip_heredocs(command)));
    let launches: Vec<(usize, String)> = parsed
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            let words = &segment.words[shell::command(&segment.words, LAUNCH_PREFIXES)..];
            launch(words, segment.background).map(|shape| (index, shape))
        })
        .collect();
    if launches.is_empty() {
        return lexical::detached(command).into_iter().map(Hit::Detached).collect();
    }
    let joined_at = parsed.iter().rposition(|segment| {
        segment.words.get(shell::command(&segment.words, LAUNCH_PREFIXES)).map(String::as_str) == Some("wait")
    });
    launches
        .into_iter()
        .filter(|(index, shape)| {
            shape != UNMANAGED || !(joined_at.is_some_and(|wait| *index < wait) || bounded_group(&parsed, *index))
        })
        .map(|(_, shape)| Hit::Detached(shape))
        .collect()
}

fn bounded_group(parsed: &[Segment], index: usize) -> bool {
    parsed[index].group.is_some_and(|start| {
        parsed[start..index].iter().any(|segment| {
            let words = &segment.words[shell::command(&segment.words, LAUNCH_PREFIXES)..];
            matches!(words, [north, capacity, bounded, ..] if shell::basename(north) == "north" && capacity == "capacity" && bounded == "bounded")
        })
    })
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
    group: Option<usize>,
}

fn segments(tokens: &[String]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut piped = false;
    let mut opens: Vec<usize> = Vec::new();
    let mut closed: Option<usize> = None;
    for (index, token) in tokens.iter().enumerate() {
        let redirect = token == "&"
            && (words.last().is_some_and(|word| word.ends_with('>'))
                || tokens
                    .get(index + 1)
                    .is_some_and(|next| next.starts_with('>')));
        if shell::is_separator(token) && !redirect {
            let background = token == "&";
            let group = if background && words.is_empty() { closed.take() } else { None };
            out.push(Segment {
                words: std::mem::take(&mut words),
                background,
                piped,
                group,
            });
            closed = None;
            match token.as_str() {
                "(" => opens.push(out.len()),
                ")" => closed = opens.pop(),
                _ => {}
            }
            piped = matches!(token.trim_end_matches('\n'), "|" | "|&");
        } else {
            closed = None;
            words.push(token.clone());
        }
    }
    out.push(Segment {
        words,
        background: false,
        piped,
        group: None,
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
        Some("north") if args.first().map(String::as_str) == Some("capacity") && args.get(1).map(String::as_str) == Some("bounded") => None,
        _ if background => Some(UNMANAGED.into()),
        _ => None,
    }
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
