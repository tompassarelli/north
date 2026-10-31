use super::git::tracks_nothing;
use super::shell::tokens;
use super::{
    COPY_COMMANDS, DESTRUCTIVE_COMMANDS, Env, PACKAGE_MANAGERS, Policy, READ_ONLY_COMMANDS,
    REMOTE_COPY_COMMANDS, WRITE_COMMANDS,
};
use crate::shell::{self, basename};
use regex::Regex;
use std::sync::LazyLock;

fn remote_spec(word: &str) -> bool {
    word.split_once(':')
        .is_some_and(|(host, _)| !host.is_empty() && !host.contains('/') && !host.starts_with('.'))
}

pub(super) fn cargo_target_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for (segment, _) in shell::shell_segments_sep(text) {
        let words = tokens(&segment);
        let executable = shell::command_index(&words);
        if words
            .get(executable).is_none_or(|s| basename(s).trim_start_matches('(') != "cargo")
        {
            continue;
        }
        for word in &words[..executable] {
            if let Some(value) = word.strip_prefix("CARGO_TARGET_DIR=")
                && !value.is_empty()
                && !value.contains('$')
            {
                paths.push(value.into());
            }
        }
        let mut i = executable + 1;
        while i < words.len() {
            if words[i] == "--target-dir" && i + 1 < words.len() {
                let value = &words[i + 1];
                if !value.is_empty() && !value.contains('$') {
                    paths.push(value.clone());
                }
                i += 2;
                continue;
            }
            if let Some(value) = words[i].strip_prefix("--target-dir=")
                && !value.is_empty()
                && !value.contains('$')
            {
                paths.push(value.into());
            }
            i += 1;
        }
    }
    paths
}
impl Policy {
    pub(super) fn scan_write_commands(&self, words: &[String], cwd: &str, env: &Env) -> Option<String> {
        let package = words
            .first()
            .is_some_and(|s| PACKAGE_MANAGERS.contains(&basename(s)));
        let executable = shell::command_index(words);
        if words
            .get(executable)
            .is_some_and(|s| READ_ONLY_COMMANDS.contains(&basename(s)))
        {
            return None;
        }
        static REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d*(?:>>|>|<)").unwrap());
        for (i, word) in words.iter().enumerate() {
            let base = basename(word);
            if package && i > 0 {
                continue;
            }
            if !introduced(words, executable, i) {
                continue;
            }
            let args: Vec<_> = words[i + 1..]
                .iter()
                .filter(|s| !s.starts_with('-') && !REDIRECT.is_match(s))
                .collect();
            if base == "sed" && words[i + 1..].iter().any(|s| s.starts_with("-i")) {
                for arg in args.iter().skip(1) {
                    if let Some((path, _)) = self.target(arg, env, cwd)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(arg, &hit, "edit in place"));
                    }
                }
            } else if matches!(base, "perl" | "ruby")
                && words[i + 1..].iter().any(|s| interpreter_flags(s).0)
            {
                let scripts = words[i + 1..]
                    .iter()
                    .filter(|s| interpreter_flags(s).1)
                    .count()
                    .max(1);
                for arg in args.iter().skip(scripts) {
                    if let Some((path, _)) = self.target(arg, env, cwd)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(arg, &hit, "edit in place"));
                    }
                }
            } else if matches!(base, "python" | "python3") {
                for literal in python_write_targets(&words[i + 1..]) {
                    if let Some((path, _)) = self.target(&literal, env, cwd)
                        && let Some(hit) = self.protected(&path)
                    {
                        return Some(self.deny(&literal, &hit, "write a file from an inline script"));
                    }
                }
            } else if WRITE_COMMANDS.contains(&base) || DESTRUCTIVE_COMMANDS.contains(&base) {
                for arg in args {
                    let Some((path, scope)) = self.target(arg, env, cwd) else {
                        continue;
                    };
                    if DESTRUCTIVE_COMMANDS.contains(&base)
                        && let Some(pin) = self.sidecar(&path)
                        && let Some(hit) = self.protected(&pin)
                    {
                        return Some(self.deny(&path, &hit, &format!("delete live consumer metadata with `{base}`; use `north pin-retire --consumer-main CONSUMER/main -- {pin}` instead")));
                    }
                    if let Some(hit) = self.protected(&path) {
                        if DESTRUCTIVE_COMMANDS.contains(&base)
                            && hit.pin.is_none()
                            && tracks_nothing(&scope)
                        {
                            continue;
                        }
                        return Some(self.deny(arg, &hit, &format!("run `{base}`")));
                    }
                }
            } else if COPY_COMMANDS.contains(&base) && !args.is_empty() {
                if base == "mv" {
                    for source in &args[..args.len() - 1] {
                        if let Some((path, _)) = self.target(source, env, cwd)
                            && let Some(pin) = self.sidecar(&path)
                            && let Some(hit) = self.protected(&pin)
                        {
                            return Some(self.deny(&path, &hit, &format!("move live consumer metadata; use `north pin-retire --consumer-main CONSUMER/main -- {pin}` instead")));
                        }
                    }
                }
                let dest = args.last().unwrap();
                if REMOTE_COPY_COMMANDS.contains(&base) && remote_spec(dest) {
                    continue;
                }
                if let Some((path, _)) = self.target(dest, env, cwd)
                    && let Some(hit) = self.protected(&path)
                {
                    return Some(self.deny(dest, &hit, &format!("run `{base}` into")));
                }
            }
        }
        None
    }
}
fn interpreter_flags(word: &str) -> (bool, bool) {
    let Some(letters) = word.strip_prefix('-').filter(|s| !s.starts_with('-')) else {
        return (false, false);
    };
    for letter in letters.chars() {
        match letter {
            'i' => return (true, false),
            'e' | 'E' => return (false, true),
            'M' | 'm' | 'I' | 'x' | 'C' | 'F' | 'd' | 'D' | 'r' | 'K' | 'T' => break,
            _ => {}
        }
    }
    (false, false)
}
fn python_write_targets(words: &[String]) -> Vec<String> {
    static WRITES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\.(?:write|writelines|write_text|write_bytes)\s*\(|\bopen\s*\([^()]*,\s*(?:mode\s*=\s*)?['"][rbt]*[wax+]"#).unwrap()
    });
    static LITERAL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"'([^']*)'|"([^"]*)""#).unwrap());
    let Some(at) = words.iter().position(|s| s == "-c") else {
        return Vec::new();
    };
    let Some(code) = words.get(at + 1).filter(|code| WRITES.is_match(code)) else {
        return Vec::new();
    };
    LITERAL
        .captures_iter(code)
        .filter_map(|capture| capture.get(1).or_else(|| capture.get(2)))
        .map(|literal| literal.as_str().to_owned())
        .filter(|literal| literal.contains(['/', '.']))
        .chain(
            words[at + 2..]
                .iter()
                .filter(|s| !s.starts_with('-'))
                .cloned(),
        )
        .collect()
}
const INTRODUCERS: &[&str] = &[
    "xargs", "-exec", "-execdir", "-ok", "-okdir", "sudo", "doas", "env", "nice", "ionice",
    "nohup", "setsid", "timeout", "stdbuf", "command", "exec", "time", "parallel", "watch",
    "flock", "unshare", "run-bounded",
];
fn introduced(words: &[String], executable: usize, at: usize) -> bool {
    at <= executable || words[executable..at].iter().any(|s| INTRODUCERS.contains(&basename(s)))
}
