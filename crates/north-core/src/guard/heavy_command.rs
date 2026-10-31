use crate::hook::{HookInput, Verdict};
use crate::{platform::pyjson::Value, shell};
use std::path::{Path, PathBuf};

const PREFIXES: &[&str] = &[
    "exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time",
];

#[derive(Debug, PartialEq)]
pub(super) enum Decision {
    Allow,
    Route,
    Refuse(String),
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("heavy-command-guard") {
        return Verdict::Allow;
    }
    let Some(command) = input.command().and_then(|s| s.as_str()) else {
        return Verdict::Allow;
    };
    match decide(command, Path::new(input.cwd().unwrap_or(".")), false, 0) {
        Decision::Allow => Verdict::Allow,
        Decision::Refuse(reason) => Verdict::Deny(reason),
        Decision::Route => {
            let wrapped = format!(
                "bun \"$(dirname \"$(agents path machine-capacity)\")/scripts/machine-capacity.mjs\" run --owner heavy-command-guard --timeout-seconds 1200 -- bash -c {}",
                quote(command)
            );
            let mut updated = input.tool_input.clone();
            let field = match &updated["command"] {
                Value::String(_) | Value::Array(_) => "command",
                _ => "cmd",
            };
            let wrapped = Value::from(wrapped);
            updated[field] = if updated[field].as_array().is_some() {
                Value::Array(vec!["bash".into(), "-lc".into(), wrapped])
            } else {
                wrapped
            };
            Verdict::Rewrite(
                updated,
                "Heavy command routed through machine-capacity admission.".into(),
            )
        }
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
fn join(words: &[String]) -> String {
    words.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
}

fn capacity(words: &[String]) -> Option<usize> {
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
    let Some(tokens) = shell::words(&text) else {
        return Decision::Allow;
    };
    let mut cwd = cwd.to_path_buf();
    let mut result = Decision::Allow;
    for body in shell::shell_substitutions(&text) {
        match decide(&body, &cwd, leased, depth + 1) {
            refused @ Decision::Refuse(_) => return refused,
            Decision::Route => result = Decision::Route,
            Decision::Allow => {}
        }
    }
    for segment in shell::segments(&tokens) {
        let words = &segment[shell::command(&segment, PREFIXES)..];
        if words.is_empty() {
            continue;
        }
        if shell::basename(&words[0]) == "cd" && words.len() == 2 {
            cwd = cwd.join(&words[1]);
            continue;
        }
        let decision = segment_decision(words, &cwd, leased, depth);
        match decision {
            Decision::Refuse(_) => return decision,
            Decision::Route => result = decision,
            Decision::Allow => {}
        }
    }
    if !leased && cpu_redirect(source) {
        return refuse("only machine-capacity may throttle CPU");
    }
    result
}

fn segment_decision(words: &[String], cwd: &Path, leased: bool, depth: usize) -> Decision {
    if let Some((source, directory, lease)) = nested(words, cwd, leased) {
        decide(&source, &directory, lease, depth + 1)
    } else if let Some(reason) = farm_only(words, cwd) {
        Decision::Refuse(reason)
    } else if !leased && cpu_throttle(words) {
        refuse("only machine-capacity may throttle CPU")
    } else if !leased {
        classify(words)
    } else {
        Decision::Allow
    }
}

fn nested(words: &[String], cwd: &Path, leased: bool) -> Option<(String, PathBuf, bool)> {
    let name = shell::basename(&words[0]);
    if let Some(i) = capacity(words) {
        if words
            .get(i + 1)
            .is_some_and(|s| matches!(s.as_str(), "run" | "session"))
        {
            let marker = words.iter().skip(i + 2).position(|s| s == "--")? + i + 2;
            return Some((join(&words[marker + 1..]), cwd.into(), true));
        }
        return None;
    }
    if name == "nix" && words.get(1).is_some_and(|s| s == "develop") {
        let marker = words
            .iter()
            .position(|s| matches!(s.as_str(), "-c" | "--command"))?;
        return Some((join(&words[marker + 1..]), cwd.into(), leased));
    }
    if matches!(name, "bash" | "sh") {
        let marker = words
            .iter()
            .skip(1)
            .position(|s| s.starts_with('-') && s[1..].contains('c'))?
            + 1;
        return Some((words.get(marker + 1)?.clone(), cwd.into(), leased));
    }
    if name == "env" {
        let mut directory = cwd.to_path_buf();
        let mut i = 1;
        while let Some(word) = words.get(i) {
            match word.as_str() {
                "-u" | "--unset" => i += 2,
                "-C" | "--chdir" => {
                    directory = directory.join(words.get(i + 1)?);
                    i += 2;
                }
                _ if word.starts_with("--chdir=") => {
                    directory = directory.join(&word[8..]);
                    i += 1;
                }
                _ if word.starts_with('-') || word.contains('=') => i += 1,
                _ => break,
            }
        }
        return Some((join(&words[i..]), directory, leased));
    }
    None
}

fn classify(words: &[String]) -> Decision {
    let name = shell::basename(&words[0]);
    let args = &words[1..];
    let plain: Vec<_> = args
        .iter()
        .filter(|s| !s.starts_with('-'))
        .map(String::as_str)
        .collect();
    if name == "cargo" {
        return match plain.first().copied() {
            Some("build") => Decision::Route,
            Some("test") => Decision::Route,
            _ => Decision::Allow,
        };
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
        return bun_command(args, &plain);
    }
    Decision::Allow
}

fn bun_command(args: &[String], plain: &[&str]) -> Decision {
    let plain = plain.strip_prefix(&["run"]).unwrap_or(plain);
    if plain.first() == Some(&"test") {
        return bun_tests(args);
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

fn bun_tests(args: &[String]) -> Decision {
    let targets = test_targets(args);
    match targets.as_slice() {
        [] => refuse("bun test (full suite)"),
        [file] if !file.contains(['*', '?', '[']) && script_file(file) => Decision::Allow,
        _ => Decision::Route,
    }
}

fn script_file(path: &str) -> bool {
    [".ts", ".js", ".tsx", ".jsx"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn test_targets(args: &[String]) -> Vec<&str> {
    let mut skip = false;
    args.iter()
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
            !arg.starts_with('-') && (arg.contains('/') || script_file(arg))
        })
        .map(String::as_str)
        .collect()
}

fn refuse(command: &str) -> Decision {
    Decision::Refuse(format!(
        "{command} requires capacity admission; use machine-capacity run --owner OWNER --timeout-seconds 1200 -- COMMAND ARG..."
    ))
}

fn cpu_property(value: &str) -> bool {
    [
        "CPUQuota=",
        "CPUQuotaPeriodSec=",
        "CPUWeight=",
        "StartupCPUWeight=",
        "AllowedCPUs=",
    ]
    .iter()
    .any(|p| value.starts_with(p))
}

fn cpu_file(path: &str) -> bool {
    path.starts_with("/sys/fs/cgroup/")
        && matches!(
            shell::basename(path),
            "cpu.max" | "cpu.weight" | "cpu.weight.nice"
        )
}

fn cpu_throttle(words: &[String]) -> bool {
    let args = &words[1..];
    match shell::basename(&words[0]) {
        "systemctl" => {
            args.iter()
                .find(|s| !s.starts_with('-'))
                .is_some_and(|s| s == "set-property")
                && args.iter().any(|s| cpu_property(s))
        }
        "systemd-run" => args
            .iter()
            .take_while(|s| s.as_str() != "--")
            .any(|s| cpu_property(s.trim_start_matches("--property=").trim_start_matches("-p"))),
        "tee" => args.iter().any(|s| cpu_file(s)),
        _ => false,
    }
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

fn farm_only(words: &[String], cwd: &Path) -> Option<String> {
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
                .map(String::as_str)
                .eq(leading)
            && rule
                .pattern
                .iter()
                .filter(|s| s.starts_with('-'))
                .all(|s| flags.contains(s))
            && !rule.narrowing.iter().any(|s| flags.contains(s))
        {
            return Some(format!("No. {}. Run on the farm: {}", rule.why, rule.farm));
        }
    }
    None
}

struct FarmRule<'a> {
    pattern: Vec<&'a str>,
    narrowing: Vec<&'a str>,
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
    let narrowing = match entry.get("narrowing") {
        None => Vec::new(),
        Some(value) => value
            .as_array()?
            .iter()
            .map(|s| s.as_str())
            .collect::<Option<Vec<_>>>()?,
    };
    Some(FarmRule {
        pattern,
        narrowing,
        farm: entry.get("farm")?.as_str()?,
        why: entry.get("why")?.as_str()?,
    })
}

#[cfg(test)]
#[path = "heavy_command_tests.rs"]
mod tests;
