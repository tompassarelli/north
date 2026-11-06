//! The part of a command that changes files or the git index, which an offloaded build would run on the build host instead of in this worktree.

use super::{PREFIXES, capacity};
use crate::nix_command::{self, Inner};
use crate::shell;

const WRITERS: &[&str] = &[
    "mv", "cp", "rm", "rmdir", "touch", "mkdir", "ln", "chmod", "chown", "patch", "truncate",
    "install", "dd", "rsync", "unzip", "rustfmt",
];

const GIT_READS: &[&str] = &[
    "status",
    "diff",
    "log",
    "show",
    "rev-parse",
    "ls-files",
    "grep",
    "blame",
    "describe",
    "merge-base",
    "cat-file",
    "rev-list",
    "shortlog",
];

/// The first segment of `source` that writes files or the git index, outside any explicit capacity run.
pub(super) fn first(source: &str, depth: usize) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let text = shell::strip_heredocs(source);
    shell::shell_substitutions(&text)
        .iter()
        .find_map(|body| first(body, depth + 1))
        .or_else(|| {
            shell::shell_segments_sep(&text)
                .into_iter()
                .find_map(|(segment, _)| writes(&segment, depth).then_some(segment))
        })
}

fn writes(segment: &str, depth: usize) -> bool {
    let Some(words) = shell::words(segment) else {
        return false;
    };
    let words = &words[shell::command(&words, PREFIXES)..];
    if words.is_empty() || capacity(words).is_some() {
        return false;
    }
    if let Some(Inner::Script(source)) = nix_command::unwrap(words) {
        return first(source, depth + 1).is_some();
    }
    redirects_to_file(segment) || writer(words)
}

fn redirects_to_file(segment: &str) -> bool {
    let tokens = shell::tokenize(segment, ";&|()\n<>", false, false, " \t\r");
    tokens.windows(2).any(|pair| {
        pair[0].0 == "sep" && pair[0].1 == ">" && pair[1].0 == "word" && pair[1].1 != "/dev/null"
    })
}

fn writer(words: &[String]) -> bool {
    let args = &words[1..];
    match shell::basename(&words[0]) {
        "sed" | "perl" => args.iter().any(|arg| {
            arg.starts_with("--in-place")
                || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('i'))
        }),
        "tee" => args.iter().any(|arg| !arg.starts_with('-')),
        "git" => git_subcommand(args).is_some_and(|subcommand| !GIT_READS.contains(&subcommand)),
        "cargo" => args.iter().any(|arg| arg == "fmt") && !args.iter().any(|arg| arg == "--check"),
        name => WRITERS.contains(&name),
    }
}

fn git_subcommand(args: &[String]) -> Option<&str> {
    let mut words = args.iter().map(String::as_str);
    loop {
        match words.next()? {
            "-C" | "-c" | "--git-dir" | "--work-tree" => {
                words.next();
            }
            word if word.starts_with('-') => {}
            word => return Some(word),
        }
    }
}
