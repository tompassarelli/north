use super::argument_words::{is_plain_argument, is_redirect_operator};
use self::cpu::{cpu_file, cpu_throttle};
use crate::hook_input::{HookInput, Verdict};
use crate::nix_command::{self, Inner};
use crate::{platform::pyjson::Value, shell};
use std::path::{Path, PathBuf};

mod bounded;
mod cpu;
mod local_write;

const PREFIXES: &[&str] = &[
    "exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time",
];

#[derive(Debug, PartialEq)]
pub(super) enum Decision {
    Allow,
    Bound,
    Route,
    Refuse(String),
}

pub fn judge(input: &HookInput) -> Verdict {
    let Some(command) = input.command().and_then(|s| s.as_str()) else {
        return Verdict::Allow;
    };
    match decide(command, Path::new(input.cwd().unwrap_or(".")), false, 0) {
        Decision::Allow => Verdict::Allow,
        Decision::Refuse(reason) => Verdict::Deny(reason),
        Decision::Route if let Some(segment) = local_write::first(command, 0) => Verdict::Deny(format!(
            "`{segment}` changes files or the git index, and an offloaded build runs the whole command on the build host, so that change would not reach this worktree. Run it as its own command first, then the build alone."
        )),
        Decision::Route => Verdict::Rewrite(
            replaced(input, format!("north capacity host run --owner heavy-command-guard --timeout-seconds 1200 -- bash -c {}", quote(command))),
            "Heavy command routed through machine-capacity admission.".into(),
        ),
        Decision::Bound => Verdict::Rewrite(replaced(input, bounded::wrapped(&quote(command))), bounded::NOTE.into()),
    }
}

fn replaced(input: &HookInput, command: String) -> Value {
    let mut updated = input.tool_input.clone();
    let field = match &updated["command"] {
        Value::String(_) | Value::Array(_) => "command",
        _ => "cmd",
    };
    let command = Value::from(command);
    updated[field] = if updated[field].as_array().is_some() {
        Value::Array(vec!["bash".into(), "-lc".into(), command])
    } else {
        command
    };
    updated
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
fn join(words: &[String]) -> String {
    words.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
}

fn capacity(words: &[String]) -> Option<usize> {
    if let [north, capacity, host, ..] = words
        && shell::basename(north) == "north"
        && capacity == "capacity"
        && host == "host"
    {
        return Some(2);
    }
    words.iter().take(3).enumerate().find_map(|(i, word)| {
        (matches!(
            shell::basename(word),
            "machine-capacity" | "machine-capacity.mjs"
        ) && (i == 0
            || (matches!(shell::basename(&words[0]), "bun" | "node" | "nodejs")
                && words[1..i].iter().all(|s| s.starts_with('-')))))
        .then_some(i)
    })
}

pub(super) fn decide(source: &str, cwd: &Path, leased: bool, depth: usize) -> Decision {
    if depth > 8 {
        return Decision::Allow;
    }
    let text = shell::strip_heredocs(source);
    let Some(tokens) = shell::words(&collapse_substitutions(&text)) else {
        return Decision::Allow;
    };
    let mut cwd = cwd.to_path_buf();
    let (mut refusals, mut pace) = (Vec::new(), Pace::Free);
    for body in shell::shell_substitutions(&text) {
        fold(decide(&body, &cwd, leased, depth + 1), &mut refusals, &mut pace);
    }
    for segment in shell::segments(&tokens) {
        let start = shell::command(&segment, PREFIXES);
        let words = &segment[start..];
        if words.is_empty() {
            continue;
        }
        let set = assigned(&segment[..start]);
        if shell::basename(&words[0]) == "cd" && words.len() == 2 {
            cwd = cwd.join(&words[1]);
            continue;
        }
        fold(segment_decision(words, &set, &cwd, leased, depth), &mut refusals, &mut pace);
    }
    if !leased && cpu_redirect(source) {
        fold(refuse("only machine-capacity may throttle CPU; to lower agent.slice now without admission, run north capacity mode relieve PERCENT [--minutes N]"), &mut refusals, &mut pace);
    }
    settled(refusals, pace)
}

fn collapse_substitutions(text: &str) -> String {
    let mut out = String::new();
    let (mut depth, mut single) = (0usize, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' if depth == 0 => {
                single = !single;
                out.push(c);
            }
            '$' if !single && depth == 0 && chars.peek() == Some(&'(') => {
                chars.next();
                depth = 1;
                out.push('x');
            }
            '(' if depth > 0 => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => out.push(c),
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Pace {
    Free,
    Bounded,
    Routed,
}

fn settled(refusals: Vec<String>, pace: Pace) -> Decision {
    match pace {
        _ if !refusals.is_empty() => Decision::Refuse(refusals.join("\n")),
        Pace::Routed => Decision::Route,
        Pace::Bounded => Decision::Bound,
        Pace::Free => Decision::Allow,
    }
}

fn fold(decision: Decision, refusals: &mut Vec<String>, pace: &mut Pace) {
    match decision {
        Decision::Refuse(reason) if !refusals.contains(&reason) => refusals.push(reason),
        Decision::Route => *pace = Pace::Routed,
        Decision::Bound => *pace = (*pace).max(Pace::Bounded),
        Decision::Refuse(_) | Decision::Allow => {}
    }
}

fn segment_decision(words: &[String], set: &[&str], cwd: &Path, leased: bool, depth: usize) -> Decision {
    let Some(commands) = nix_command::launched(words) else {
        return own_decision(words, set, cwd, leased, depth);
    };
    let (mut refusals, mut pace) = (Vec::new(), Pace::Free);
    fold(own_decision(words, set, cwd, leased, depth), &mut refusals, &mut pace);
    for command in commands {
        fold(segment_decision(command, set, cwd, leased, depth + 1), &mut refusals, &mut pace);
    }
    settled(refusals, pace)
}

fn own_decision(words: &[String], set: &[&str], cwd: &Path, leased: bool, depth: usize) -> Decision {
    if let Some(nested) = nested(words, cwd, leased) {
        match nested {
            Nested::Source(source, directory, lease) => decide(&source, &directory, lease, depth + 1),
            Nested::NixBuild if leased => Decision::Allow,
            Nested::NixBuild => Decision::Route,
        }
    } else if let Some(reason) = farm_only(words, set, cwd) {
        Decision::Refuse(reason)
    } else if !leased && cpu_throttle(words) {
        refuse("only machine-capacity may throttle CPU; to lower agent.slice now without admission, run north capacity mode relieve PERCENT [--minutes N]")
    } else if !leased {
        classify(words, cwd)
    } else {
        Decision::Allow
    }
}

enum Nested {
    Source(String, PathBuf, bool),
    NixBuild,
}

fn nested(words: &[String], cwd: &Path, leased: bool) -> Option<Nested> {
    if let Some(i) = capacity(words) {
        if words
            .get(i + 1)
            .is_some_and(|s| matches!(s.as_str(), "run" | "session"))
        {
            let marker = words.iter().skip(i + 2).position(|s| s == "--")? + i + 2;
            return Some(Nested::Source(join(&words[marker + 1..]), cwd.into(), true));
        }
        return None;
    }
    Some(match nix_command::unwrap(words)? {
        Inner::Command { words, directories } => Nested::Source(
            join(&words),
            directories.iter().fold(cwd.to_path_buf(), |directory, step| directory.join(step)),
            leased,
        ),
        Inner::Script(source) => Nested::Source(source.to_owned(), cwd.into(), leased),
        Inner::NixBuild => Nested::NixBuild,
    })
}

fn classify(words: &[String], cwd: &Path) -> Decision {
    let name = shell::basename(&words[0]);
    let args = &words[1..];
    let plain: Vec<_> = args
        .iter()
        .filter(|s| !s.starts_with('-'))
        .map(String::as_str)
        .collect();
    if name == "cargo" {
        return cargo_command(args);
    }
    if name == "cargo-nextest" || name == "rustc" && args.iter().any(|s| s.ends_with(".rs")) {
        return Decision::Route;
    }
    if name == "tsc" && args.iter().any(|s| matches!(s.as_str(), "--build" | "-b")) {
        return Decision::Route;
    }
    if matches!(name, "bunx" | "npx")
        && plain.first() == Some(&"tsc")
        && args.iter().any(|s| matches!(s.as_str(), "--build" | "-b"))
    {
        return Decision::Route;
    }
    if name == "bun" {
        return bun_command(args, &plain, cwd);
    }
    Decision::Allow
}

/// Cargo subcommands that invoke rustc, with their built-in aliases.
const CARGO_COMPILING: &[&str] = &[
    "build", "b", "check", "c", "clippy", "test", "t", "nextest", "bench", "doc", "d", "run", "r",
    "rustc", "rustdoc", "fix", "install", "llvm-cov", "mutants", "miri",
];

/// Cargo global options whose value is the next word.
const CARGO_VALUED: &[&str] = &["-C", "--manifest-path", "--config", "-Z", "--color"];

fn cargo_command(args: &[String]) -> Decision {
    let mut words = args.iter().map(String::as_str);
    let subcommand = loop {
        match words.next() {
            None => return Decision::Allow,
            Some(word) if CARGO_VALUED.contains(&word) => {
                words.next();
            }
            Some(word) if word.starts_with('-') || word.starts_with('+') => {}
            Some(word) => break word,
        }
    };
    if CARGO_COMPILING.contains(&subcommand) {
        Decision::Route
    } else {
        Decision::Allow
    }
}

fn bun_command(args: &[String], plain: &[&str], cwd: &Path) -> Decision {
    let runner = plain.first() == Some(&"test");
    let plain = plain.strip_prefix(&["run"]).unwrap_or(plain);
    if plain.first() == Some(&"test") {
        return bun_tests(args, runner.then_some(cwd));
    }
    if plain.starts_with(&["wisp", "map", "build"]) {
        return Decision::Route;
    }
    if plain.starts_with(&["wisp", "view"])
        || (plain.starts_with(&["wisp", "headless"]) && args.iter().any(|s| s == "--render"))
    {
        return Decision::Route;
    }
    if plain.first() == Some(&"tsc") && args.iter().any(|s| matches!(s.as_str(), "--build" | "-b"))
    {
        return Decision::Route;
    }
    Decision::Allow
}

fn bun_tests(args: &[String], runner: Option<&Path>) -> Decision {
    let targets = test_targets(args);
    match targets.as_slice() {
        [] if runner.is_some_and(bounded::light_suite) => Decision::Bound,
        [] => refuse("bun test (full suite)"),
        [file] if !file.contains(['*', '?', '[']) && script_file(file) => Decision::Allow,
        _ => Decision::Route,
    }
}

fn script_file(path: &str) -> bool {
    [".ts", ".js", ".tsx", ".jsx", ".mjs", ".cjs", ".mts", ".cts"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn test_targets(args: &[String]) -> Vec<&str> {
    let mut skip = false;
    args.iter()
        .skip_while(|arg| *arg != "test")
        .skip(1)
        .filter(|arg| {
            if skip {
                skip = false;
                return false;
            }
            if matches!(
                arg.as_str(),
                "--timeout"
                    | "--rerun-each"
                    | "--retry"
                    | "--seed"
                    | "--test-name-pattern"
                    | "-t"
                    | "--reporter"
                    | "--reporter-outfile"
                    | "--preload"
                    | "--coverage-dir"
                    | "--max-concurrency"
            ) {
                skip = true;
                return false;
            }
            if is_redirect_operator(arg) {
                skip = true;
                return false;
            }
            is_plain_argument(arg)
        })
        .map(String::as_str)
        .collect()
}

fn refuse(command: &str) -> Decision {
    Decision::Refuse(format!(
        "{command} requires capacity admission; use machine-capacity run --owner OWNER --timeout-seconds 1200 -- COMMAND ARG..."
    ))
}

fn cpu_redirect(source: &str) -> bool {
    shell::shell_segments_sep(&shell::strip_heredocs(source))
        .iter()
        .any(|(segment, _)| {
            let Some(words) = shell::words(segment) else {
                return false;
            };
            let words = &words[shell::command(&words, PREFIXES)..];
            if capacity(words).is_some() {
                return false;
            }
            let tokens = shell::tokenize(segment, ";&|()\n<>", false, false, " \t\r");
            let tokens = &tokens[..tokens
                .iter()
                .position(|(kind, word, quoted)| {
                    *kind == "word" && !quoted && word.starts_with('#')
                })
                .unwrap_or(tokens.len())];
            tokens.windows(2).any(|pair| {
                pair[0].0 == "sep"
                    && pair[0].1 == ">"
                    && pair[1].0 == "word"
                    && cpu_file(&pair[1].1)
            })
        })
}

fn assigned(prefix: &[String]) -> Vec<&str> {
    prefix
        .iter()
        .filter(|s| shell::assignment(s))
        .filter_map(|s| s.split_once('='))
        .filter(|(_, value)| !value.is_empty())
        .map(|(name, _)| name)
        .collect()
}

fn script_word(word: &str) -> &str {
    let mut word = word;
    while let Some(rest) = word.strip_prefix("./") {
        word = rest;
    }
    word
}

fn farm_only(words: &[String], set: &[&str], cwd: &Path) -> Option<String> {
    let args = if words.get(1).is_some_and(|s| s == "run") {
        &words[2..]
    } else {
        &words[1..]
    };
    if shell::basename(&words[0]) == "bun"
        && args.first().is_some_and(|s| s == "test")
        && !test_targets(args).is_empty()
    {
        return None;
    }
    let cwd = std::fs::canonicalize(cwd).ok()?;
    let directory = cwd.ancestors().find(|p| p.join(".git").exists())?;
    let text = std::fs::read_to_string(directory.join(".agents/farm-only.toml")).ok()?;
    let document = text.parse::<toml_edit::DocumentMut>().ok()?;
    let entries = document
        .get("command")?
        .as_array_of_tables()?
        .iter()
        .map(farm_rule)
        .collect::<Option<Vec<_>>>()?;
    for rule in entries {
        let leading: Vec<_> = rule
            .pattern
            .iter()
            .filter(|s| !s.starts_with('-'))
            .copied()
            .collect();
        let flags: Vec<_> = words
            .iter()
            .skip(leading.len())
            .filter(|s| s.starts_with('-'))
            .map(|s| s.split('=').next().unwrap_or(s))
            .collect();
        if !leading.is_empty()
            && words
                .iter()
                .take(leading.len())
                .map(|s| script_word(s))
                .eq(leading.iter().map(|s| script_word(s)))
            && rule
                .pattern
                .iter()
                .filter(|s| s.starts_with('-'))
                .all(|s| flags.contains(s))
            && !rule.narrowing.iter().any(|s| flags.contains(s))
            && !rule.narrowing_env.iter().any(|s| set.contains(s))
        {
            return Some(format!("No. {}. Run on the farm: {}", rule.why, rule.farm));
        }
    }
    None
}

struct FarmRule<'a> {
    pattern: Vec<&'a str>,
    narrowing: Vec<&'a str>,
    narrowing_env: Vec<&'a str>,
    farm: &'a str,
    why: &'a str,
}

fn farm_rule(entry: &toml_edit::Table) -> Option<FarmRule<'_>> {
    let pattern = entry
        .get("pattern")?
        .as_array()?
        .iter()
        .map(|s| s.as_str().filter(|s| !s.is_empty()))
        .collect::<Option<Vec<_>>>()?;
    if pattern.is_empty() {
        return None;
    }
    let words = |key: &str| match entry.get(key) {
        None => Some(Vec::new()),
        Some(value) => value
            .as_array()?
            .iter()
            .map(|s| s.as_str())
            .collect::<Option<Vec<_>>>(),
    };
    let narrowing = words("narrowing")?;
    let narrowing_env = words("narrowing_env")?;
    Some(FarmRule {
        pattern,
        narrowing,
        narrowing_env,
        farm: entry.get("farm")?.as_str()?,
        why: entry.get("why")?.as_str()?,
    })
}

