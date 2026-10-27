use crate::shell;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const WRITES: &[&str] = &[
    "create", "comment", "edit", "review", "close", "reopen", "merge", "upload",
];
const FILE_FLAGS: &[&str] = &["--body-file", "-F", "--input", "--field", "--raw-field", "-f"];
const FILE_FLAG_PREFIXES: &[&str] = &["--body-file=", "--input=", "--field="];
const FIELD_FLAGS: &[&str] = &["-F", "--field", "-f", "--raw-field"];

static GH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[\n;&|({`])\s*(?:[A-Za-z_]\w*=\S*\s+|sudo\s+|env\s+|command\s+)*(?:\S*/)?gh((?:\s+(?:-R|--repo)\s+\S+)*)\s+(issue|pr|api|gist|release)\b([^\n;&|)}`]*)",
    )
    .unwrap()
});

pub(in crate::guard) fn writes_to_github(command: &str) -> bool {
    if !command.contains("gh") {
        return false;
    }
    let cleaned = shell::strip_quotes(&shell::strip_heredocs(command));
    GH.captures_iter(&cleaned).any(|found| {
        matches!(&found[2], "api" | "gist")
            || found[3]
                .split_whitespace()
                .next()
                .is_some_and(|verb| WRITES.contains(&verb))
    })
}

pub(super) fn body_files(command: &str, cwd: &Path) -> Vec<PathBuf> {
    let words = shell::words(command).unwrap_or_default();
    let mut files = Vec::new();
    for segment in shell::segments(&words) {
        if !segment.iter().any(|word| shell::basename(word) == "gh") {
            continue;
        }
        for (index, word) in segment.iter().enumerate() {
            let next = segment.get(index + 1).map_or("", String::as_str);
            let (flag, value) = if FILE_FLAGS.contains(&word.as_str()) {
                (word.as_str(), next)
            } else if let Some(prefix) = FILE_FLAG_PREFIXES.iter().find(|p| word.starts_with(**p)) {
                (*prefix, &word[prefix.len()..])
            } else {
                continue;
            };
            let value = if FIELD_FLAGS.contains(&flag) || flag == "--field=" {
                match value.split_once("=@") {
                    Some((_, path)) => path,
                    None => continue,
                }
            } else {
                value
            };
            if !value.is_empty() && value != "-" {
                files.push(cwd.join(expand_home(value)));
            }
        }
    }
    files
}

fn expand_home(path: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (path.strip_prefix("~/"), path, home) {
        (Some(rest), _, Some(home)) => home.join(rest),
        (_, "~", Some(home)) => home,
        _ => PathBuf::from(path),
    }
}
