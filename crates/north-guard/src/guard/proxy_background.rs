//! The proxy stays free to answer: a foreground call that can wait past 30 s on a launch, a remote
//! command or a job is refused, and the proxy runs it with `run_in_background: true` instead, so a
//! report reaches it while its own work runs (north#1048).
use crate::hook_input::{HookInput, Verdict};
use north_base::shell::{basename, command as command_start, segments, strip_heredocs, words};
use serde_json::Value;

const UNIT: &str = "proxy-background-guard";
const LIMIT_SECONDS: f64 = 30.0;
const MAX_DEPTH: usize = 3;
const PREFIXES: &[&str] = &["env", "exec", "time", "timeout", "nice", "ionice", "builtin", "nohup", "setsid", "stdbuf"];
const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash"];
const SSH_QUERIES: &[&str] = &["-O", "-Q", "-V"];
const SSH_VALUED: &[&str] = &["-B", "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p", "-Q", "-R", "-S", "-W", "-w"];

pub fn evaluate(input: &HookInput, status_verb: &dyn Fn(&[&str]) -> bool) -> Verdict {
    match refusal(input, status_verb) {
        Some(reason) => Verdict::Deny(reason),
        _ => Verdict::Allow,
    }
}

/// The refusal for a proxy's foreground call that can hold its turn open, or `None` when the call is
/// backgrounded, bounded below the limit, a status read, or does no waiting.
pub(super) fn refusal(input: &HookInput, status_verb: &dyn Fn(&[&str]) -> bool) -> Option<String> {
    if !input.is_shell() || input.runs_in_background() {
        return None;
    }
    let bounded = timeout_ms(&input.tool_input["timeout"]).is_some_and(|seconds| seconds <= LIMIT_SECONDS);
    if bounded {
        return None;
    }
    waits(input.command()?, 0, status_verb).map(message)
}

fn message(what: &str) -> String {
    format!(
        "BLOCKED ({UNIT}): the proxy stays free to answer, so it does not hold its turn open in the foreground on {what}, which can wait longer than {LIMIT_SECONDS:.0} s. Run it with run_in_background: true and read the completion note it sends, whether that is a `north fleet launch`, a remote `ssh` command or a `north job-run` job; the report it produces then never waits behind the proxy's own work. Foreground sends, gates and reads stay open."
    )
}

/// What the foreground command waits on, found through its segments and the shells it wraps.
fn waits(text: &str, depth: usize, status_verb: &dyn Fn(&[&str]) -> bool) -> Option<&'static str> {
    if depth > MAX_DEPTH {
        return None;
    }
    let text = strip_heredocs(text);
    let tokens = words(&text).unwrap_or_else(|| text.split_whitespace().map(str::to_owned).collect());
    for segment in segments(&tokens) {
        if bounded_by_timeout(&segment) {
            continue;
        }
        let start = command_start(&segment, PREFIXES);
        let Some((program, args)) = segment.get(start..).and_then(<[String]>::split_first) else { continue };
        let program = basename(program);
        if program == "ssh" && !args.iter().any(|word| SSH_QUERIES.contains(&word.as_str())) {
            if let Some(what) = remote_waits(args, depth, status_verb) {
                return Some(what);
            }
            continue;
        }
        if let Some(what) = wait_class(program, args, depth, status_verb) {
            return Some(what);
        }
    }
    None
}

/// What a foreground ssh waits on: the remote command's own launch or job, else a remote command that
/// is more than the fleet, threads, gate and status verbs.
fn remote_waits(args: &[String], depth: usize, status_verb: &dyn Fn(&[&str]) -> bool) -> Option<&'static str> {
    let remote = ssh_remote(args);
    if remote.is_empty() {
        return Some("a remote command");
    }
    if let Some(what) = waits(&remote, depth + 1, status_verb) {
        return Some(what);
    }
    (!super::proxy_reach::status_only(&remote, status_verb)).then_some("a remote command")
}

fn ssh_remote(args: &[String]) -> String {
    let mut rest = args.iter();
    loop {
        let Some(word) = rest.next() else { return String::new() };
        match word.as_str() {
            word if SSH_VALUED.contains(&word) => {
                rest.next();
            }
            word if word.starts_with('-') => {}
            _ => break,
        }
    }
    rest.skip_while(|word| *word == "--").map(String::as_str).collect::<Vec<_>>().join(" ")
}

fn wait_class(program: &str, args: &[String], depth: usize, status_verb: &dyn Fn(&[&str]) -> bool) -> Option<&'static str> {
    if args.iter().any(|word| matches!(word.as_str(), "--help" | "-h")) {
        return None;
    }
    match program {
        "north" => north_wait(args),
        "scp" | "rsync" => Some("a remote copy"),
        _ if SHELLS.contains(&program) => script(args).and_then(|script| waits(script, depth + 1, status_verb)),
        _ => None,
    }
}

fn north_wait(args: &[String]) -> Option<&'static str> {
    let mut words = args.iter().map(String::as_str).filter(|word| !word.starts_with('-'));
    match (words.next()?, words.next()) {
        ("fleet", Some("launch" | "revive" | "relaunch" | "dispatch")) => Some("a fleet launch"),
        ("cloud", Some("launch")) => Some("a cloud launch"),
        ("job-run", _) if !args.iter().any(|word| word == "--notify") => Some("a job"),
        _ => None,
    }
}

fn script(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|arg| arg == "-c" || (arg.starts_with('-') && !arg.starts_with("--") && arg.ends_with('c')))?;
    args.get(at + 1).map(String::as_str)
}

fn bounded_by_timeout(segment: &[String]) -> bool {
    segment
        .iter()
        .position(|word| basename(word) == "timeout")
        .and_then(|at| segment.get(at + 1))
        .and_then(|word| duration_seconds(word))
        .is_some_and(|seconds| seconds <= LIMIT_SECONDS)
}

fn duration_seconds(word: &str) -> Option<f64> {
    let (number, scale) = match word.chars().last()? {
        's' => (&word[..word.len() - 1], 1.0),
        'm' => (&word[..word.len() - 1], 60.0),
        'h' => (&word[..word.len() - 1], 3600.0),
        'd' => (&word[..word.len() - 1], 86_400.0),
        '0'..='9' | '.' => (word, 1.0),
        _ => return None,
    };
    number.parse::<f64>().ok().map(|value| value * scale)
}

fn timeout_ms(timeout: &Value) -> Option<f64> {
    timeout.as_f64().map(|milliseconds| milliseconds / 1000.0)
}
