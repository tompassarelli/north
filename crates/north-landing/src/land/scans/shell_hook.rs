use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::{Range, git};

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "!", "[", "[[", "]]",
];

const BUILTINS: &[&str] = &[
    ":", ".", "alias", "bg", "bind", "break", "builtin", "caller", "cd", "command", "compgen",
    "complete", "continue", "declare", "dirs", "disown", "echo", "enable", "eval", "exec",
    "export", "false", "fc", "fg", "getopts", "hash", "help", "history", "jobs", "kill", "let",
    "local", "mapfile", "popd", "printf", "pushd", "pwd", "read", "readarray", "readonly",
    "return", "set", "shift", "shopt", "source", "test", "trap", "true", "type", "umask",
    "unset", "wait",
];

const STDENV: &[&str] = &[
    "awk", "bash", "basename", "bzip2", "cat", "chmod", "cmp", "cp", "cut", "date", "diff",
    "dirname", "env", "expr", "file", "find", "gawk", "grep", "gzip", "head", "id", "install",
    "ln", "ls", "make", "mkdir", "mktemp", "mv", "patch", "patchelf", "readlink", "realpath",
    "rm", "rmdir", "sed", "sh", "sleep", "sort", "tail", "tar",
];

const STDENV_MORE: &[&str] = &["tee", "touch", "tr", "uname", "uniq", "wc", "xargs"];

const RUST_TOOLCHAIN: &[&str] = &["cargo", "rustc", "rustfmt", "rustdoc", "clippy-driver"];

static OR_TRUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\|\|\s*(true|:)\s*(;|&|\)|$)").unwrap());
static EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[;&|({]|\bthen\b|\belse\b|\bdo\b)\s*exit\b").unwrap());
static PACKAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpkgs\.([A-Za-z][\w+-]*)").unwrap());
static EXPANSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{[^}]*\}").unwrap());
static PLAIN_STRING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""[^"$`]*"|'[^']*'"#).unwrap());
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z][\w.+-]*$").unwrap());
static HOOK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bshellHook\s*=\s*(''|")"#).unwrap());

fn shell_hooks(flake: &str) -> Vec<String> {
    HOOK.captures_iter(flake)
        .filter_map(|opening| {
            let whole = opening.get(0)?;
            let rest = &flake[whole.end()..];
            let body = if &opening[1] == "\"" { double_quoted(rest) } else { indented(rest) };
            body.map(str::to_owned)
        })
        .collect()
}

fn indented(rest: &str) -> Option<&str> {
    let bytes = rest.as_bytes();
    let mut at = 0;
    while at + 1 < bytes.len() {
        if bytes[at] == b'\'' && bytes[at + 1] == b'\'' {
            match bytes.get(at + 2) {
                Some(b'\'' | b'$' | b'\\') => at += 3,
                _ => return Some(&rest[..at]),
            }
        } else {
            at += 1;
        }
    }
    None
}

fn double_quoted(rest: &str) -> Option<&str> {
    let mut escaped = false;
    for (at, character) in rest.char_indices() {
        match character {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some(&rest[..at]),
            _ => escaped = false,
        }
    }
    None
}

fn provided(flake: &str) -> BTreeSet<String> {
    let mut programs: BTreeSet<String> = STDENV.iter().chain(STDENV_MORE).chain(BUILTINS).chain(KEYWORDS).map(|name| (*name).to_owned()).collect();
    programs.extend(PACKAGE.captures_iter(flake).map(|package| package[1].to_owned()));
    if flake.contains("rust-bin") || flake.contains("rustToolchain") || flake.contains("rustup") {
        programs.extend(RUST_TOOLCHAIN.iter().map(|name| (*name).to_owned()));
    }
    programs
}

fn commands(line: &str) -> Vec<String> {
    let expanded = EXPANSION.replace_all(line, "VARIABLE");
    let line = PLAIN_STRING.replace_all(&expanded, "\"\"");
    let split = line.replace("$(", "\n").replace(['`', ';', '&', '|', '(', ')', '{', '}'], "\n");
    split
        .lines()
        .filter_map(|segment| {
            let mut words = segment.split_whitespace().peekable();
            while words.next_if(|word| matches!(*word, "if" | "then" | "else" | "elif" | "do" | "while" | "until" | "!" | "time")).is_some() {}
            let word = words.next()?;
            (WORD.is_match(word) && !segment.trim_start().starts_with(&format!("{word}="))).then(|| word.to_owned())
        })
        .collect()
}

fn findings(flake: &str) -> Vec<String> {
    let provided = provided(flake);
    let mut found = Vec::new();
    for hook in shell_hooks(flake) {
        for line in hook.lines().filter(|line| !line.trim_start().starts_with('#')) {
            let line = line.trim();
            if OR_TRUE.is_match(line) {
                found.push(format!("`{line}` hides a failure with `|| true`; run the step in the entry that enters the shell, or let it fail"));
            }
            if EXIT.is_match(line) {
                found.push(format!("`{line}` exits the shell, which fails every `nix develop -c`; return a status from the step or drop it"));
            }
            for program in commands(line).into_iter().filter(|program| !provided.contains(program)) {
                found.push(format!("`{program}` in `{line}` is not provided by the shell's packages; run it in the entry outside the shell or add its package"));
            }
        }
    }
    found.dedup();
    found
}

fn flakes(directory: &Path, base: &str, head: &str) -> Vec<String> {
    git::text(directory, &["diff", "--name-only", "--diff-filter=d", base, head, "--"])
        .unwrap_or_default()
        .lines()
        .filter(|path| Path::new(path).file_name().is_some_and(|name| name == "flake.nix"))
        .map(str::to_owned)
        .collect()
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let mut refused = false;
    for path in flakes(directory, base, head) {
        let Some(flake) = git::text(directory, &["show", &format!("{head}:{path}")]) else {
            let _ = writeln!(out, "shell-hook: cannot read {path} at {head}");
            return 1;
        };
        for finding in findings(&flake) {
            refused = true;
            let _ = writeln!(out, "shell-hook: {path}: {finding}");
        }
    }
    if refused {
        let _ = writeln!(out, "shell-hook: a shellHook is a pure function of the flake; north target-seed and other setup run in `north dev-shell run`");
    }
    u8::from(refused)
}
