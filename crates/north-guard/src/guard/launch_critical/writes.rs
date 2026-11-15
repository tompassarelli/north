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

fn literal_dir(value: &str) -> Option<String> {
    (!value.is_empty() && !value.contains('$')).then(|| value.to_owned())
}

fn segment_target_paths(words: &[String], executable: usize) -> Vec<String> {
    let assigned = words[..executable]
        .iter()
        .filter_map(|word| word.strip_prefix("CARGO_TARGET_DIR="))
        .filter_map(literal_dir);
    let mut flagged = Vec::new();
    let mut i = executable + 1;
    while i < words.len() {
        if words[i] == "--target-dir" && i + 1 < words.len() {
            flagged.extend(literal_dir(&words[i + 1]));
            i += 2;
            continue;
        }
        flagged.extend(
            words[i]
                .strip_prefix("--target-dir=")
                .and_then(literal_dir),
        );
        i += 1;
    }
    assigned.chain(flagged).collect()
}

pub(super) fn cargo_target_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for (segment, _) in shell::shell_segments_sep(text) {
        let words = tokens(&segment);
        let executable = shell::command_index(&words);
        let is_cargo = words
            .get(executable)
            .is_some_and(|s| basename(s).trim_start_matches('(') == "cargo");
        if is_cargo {
            paths.extend(segment_target_paths(&words, executable));
        }
    }
    paths
}

struct Call<'a> {
    base: &'a str,
    rest: &'a [String],
    args: Vec<&'a String>,
    cwd: &'a str,
    env: &'a Env,
}

type Writer = (fn(&Call) -> bool, fn(&Policy, &Call) -> Option<String>);

const WRITERS: &[Writer] = &[
    (
        |call| call.base == "sed" && call.rest.iter().any(|s| s.starts_with("-i")),
        Policy::sed_in_place,
    ),
    (
        |call| {
            matches!(call.base, "perl" | "ruby")
                && call.rest.iter().any(|s| interpreter_flags(s).0)
        },
        Policy::interpreter_in_place,
    ),
    (
        |call| matches!(call.base, "awk" | "gawk") && awk_in_place(call.rest),
        Policy::awk_in_place,
    ),
    (
        |call| matches!(call.base, "python" | "python3"),
        Policy::python_inline_write,
    ),
    (
        |call| WRITE_COMMANDS.contains(&call.base) || DESTRUCTIVE_COMMANDS.contains(&call.base),
        Policy::write_or_destroy,
    ),
    (
        |call| COPY_COMMANDS.contains(&call.base) && !call.args.is_empty(),
        Policy::copy_into,
    ),
];

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
            if (package && i > 0) || !introduced(words, executable, i) {
                continue;
            }
            let rest = &words[i + 1..];
            let call = Call {
                base: basename(word),
                rest,
                args: rest
                    .iter()
                    .filter(|s| !s.starts_with('-') && !REDIRECT.is_match(s))
                    .collect(),
                cwd,
                env,
            };
            let writer = WRITERS.iter().find(|(applies, _)| applies(&call));
            if let Some(reason) = writer.and_then(|(_, scan)| scan(self, &call)) {
                return Some(reason);
            }
        }
        None
    }

    fn deny_in_place<'a>(
        &self,
        call: &Call,
        mut args: impl Iterator<Item = &'a String>,
    ) -> Option<String> {
        args.find_map(|arg| {
            let (path, _) = self.target(arg, call.env, call.cwd)?;
            let hit = self.protected(&path)?;
            Some(self.deny(arg, &hit, "edit in place"))
        })
    }

    fn sed_in_place(&self, call: &Call) -> Option<String> {
        let scripts = call
            .rest
            .iter()
            .filter(|s| matches!(s.as_str(), "-e" | "--expression" | "-f" | "--file"))
            .count()
            .max(1);
        self.deny_in_place(call, call.args.iter().copied().skip(scripts))
    }

    fn interpreter_in_place(&self, call: &Call) -> Option<String> {
        let scripts = call
            .rest
            .iter()
            .filter(|s| interpreter_flags(s).1)
            .count()
            .max(1);
        self.deny_in_place(call, call.args.iter().copied().skip(scripts))
    }

    fn awk_in_place(&self, call: &Call) -> Option<String> {
        let scripts = usize::from(!call.rest.iter().any(|s| s == "-f"));
        self.deny_in_place(
            call,
            call.args
                .iter()
                .copied()
                .filter(|s| s.as_str() != "inplace")
                .skip(scripts),
        )
    }

    fn python_inline_write(&self, call: &Call) -> Option<String> {
        python_write_targets(call.rest).into_iter().find_map(|literal| {
            let (path, _) = self.target(&literal, call.env, call.cwd)?;
            let hit = self.protected(&path)?;
            Some(self.deny(&literal, &hit, "write a file from an inline script"))
        })
    }

    fn write_or_destroy(&self, call: &Call) -> Option<String> {
        call.args
            .iter()
            .find_map(|arg| self.write_or_destroy_arg(call, arg))
    }

    fn write_or_destroy_arg(&self, call: &Call, arg: &str) -> Option<String> {
        let base = call.base;
        let destructive = DESTRUCTIVE_COMMANDS.contains(&base);
        let (path, scope) = self.target(arg, call.env, call.cwd)?;
        if destructive
            && let Some(pin) = self.sidecar(&path)
            && let Some(hit) = self.protected(&pin)
        {
            return Some(self.deny(&path, &hit, &format!("delete live consumer metadata with `{base}`; use `north pin-retire --consumer-main CONSUMER/main -- {pin}` instead")));
        }
        let hit = self.protected(&path)?;
        if destructive && hit.pin.is_none() && tracks_nothing(&scope) {
            return None;
        }
        if destructive && let Some(checkout) = git_lock_checkout(&path) {
            return Some(format!("This Bash command would run `{base}` on {path}, a git lock file inside {}. Clear stale git locks with the sanctioned verb, which removes only a lock no process holds and older than 60 s, and names a live one:\n  north lane unlock {checkout}\nA lock it keeps belongs to a running git; wait for it.", hit.noun()));
        }
        Some(self.deny(arg, &hit, &format!("run `{base}`")))
    }

    fn copy_into(&self, call: &Call) -> Option<String> {
        let base = call.base;
        let (dest, sources) = call.args.split_last()?;
        if base == "mv" {
            let moved = sources.iter().find_map(|source| {
                let (path, _) = self.target(source, call.env, call.cwd)?;
                let pin = self.sidecar(&path)?;
                let hit = self.protected(&pin)?;
                Some(self.deny(&path, &hit, &format!("move live consumer metadata; use `north pin-retire --consumer-main CONSUMER/main -- {pin}` instead")))
            });
            if moved.is_some() {
                return moved;
            }
        }
        if REMOTE_COPY_COMMANDS.contains(&base) && remote_spec(dest) {
            return None;
        }
        let (path, _) = self.target(dest, call.env, call.cwd)?;
        let hit = self.protected(&path)?;
        Some(self.deny(dest, &hit, &format!("run `{base}` into")))
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
    "flock", "unshare", "bounded",
];
fn introduced(words: &[String], executable: usize, at: usize) -> bool {
    at <= executable || words[executable..at].iter().any(|s| INTRODUCERS.contains(&basename(s)))
}
fn awk_in_place(words: &[String]) -> bool {
    words.windows(2).any(|pair| pair[0] == "-i" && pair[1] == "inplace")
        || words.iter().any(|s| s == "-iinplace" || s == "--include=inplace")
}
fn git_lock_checkout(path: &str) -> Option<&str> {
    let (checkout, inside) = path.split_once("/.git/")?;
    (inside.ends_with(".lock") && !inside.contains("/../")).then_some(checkout)
}
