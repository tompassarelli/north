use super::finding::Finding;
use super::rust_text::line_comments;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

pub(super) const COMMENT: &str = "code-comment";
const FIX: &str = "delete it, or make it one line that names the determinism, engine, Lua or external-format constraint a reader would otherwise break";

static CONSTRAINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:determinis\w*|engine|lua|formats?|schema|wire|rfc ?\d+|byte[- ]order|endian\w*)\b").expect("constraint pattern")
});

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| format!("git {}: {error}", args[0]))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn added_lines(diff: &str) -> Vec<(String, BTreeSet<usize>)> {
    let mut files: Vec<(String, BTreeSet<usize>)> = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            files.push((path.to_owned(), BTreeSet::new()));
        } else if let Some(header) = line.strip_prefix("@@ ")
            && let Some(plus) = header.split(' ').find_map(|part| part.strip_prefix('+'))
            && let Some((_, added)) = files.last_mut()
        {
            let (start, count) = plus.split_once(',').unwrap_or((plus, "1"));
            let start: usize = start.parse().unwrap_or(0);
            added.extend(start..start + count.parse::<usize>().unwrap_or(0));
        }
    }
    files
}

pub(super) fn in_file(path: &str, text: &str, added: &BTreeSet<usize>) -> Vec<Finding> {
    let comments = line_comments(text);
    let lines: BTreeSet<usize> = comments.iter().map(|(line, _)| *line).collect();
    comments
        .iter()
        .filter(|(line, _)| added.contains(line))
        .filter_map(|(line, body)| {
            let block = *line > 1 && lines.contains(&(line - 1)) || lines.contains(&(line + 1));
            let reason = if block {
                "comment of several lines"
            } else if CONSTRAINT.is_match(body) {
                return None;
            } else {
                "comment names no constraint"
            };
            Some(Finding {
                rule: COMMENT,
                path: path.to_owned(),
                line: *line,
                item: body.clone(),
                text: format!("{reason} — {FIX}"),
            })
        })
        .collect()
}

pub(super) fn added(repo: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
    let diff = git(
        repo,
        &[
            "diff",
            "-U0",
            "--no-color",
            "--no-renames",
            base,
            head,
            "--",
            "*.rs",
        ],
    )?;
    let mut lines = Vec::new();
    for (path, added) in added_lines(&diff) {
        let text = git(repo, &["show", &format!("{head}:{path}")])?;
        lines.extend(in_file(&path, &text, &added).iter().map(Finding::render));
    }
    Ok(lines)
}
