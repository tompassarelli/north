use std::path::{Path, PathBuf};
use std::process::Command;

const ROOTS: &[&str] = &["docs/", "harness/", "bin/", ".claude/", "README.md", "AGENTS.md"];
const SKIPPED: &[&str] = &["docs/rationale/", "/fixtures/", ".test.", "node_modules/"];
const SEPARATORS: &[&str] = &["&&", "||", "$(", ";", "|", "(", "`"];

#[derive(Debug, Eq, PartialEq)]
pub struct Citation {
    pub path: String,
    pub line: usize,
    pub command: String,
}

fn command_word(segment: &str) -> Option<&str> {
    let segment = segment.trim_start();
    let segment = segment.strip_prefix("$ ").unwrap_or(segment);
    let segment = segment.strip_prefix("exec ").unwrap_or(segment);
    let word = segment
        .strip_prefix("north ")?
        .trim_start()
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .next()?;
    word.starts_with(|c: char| c.is_ascii_lowercase()).then_some(word)
}

fn in_command_position(text: &str) -> Vec<&str> {
    let mut segments = vec![text];
    for separator in SEPARATORS {
        segments = segments.into_iter().flat_map(|segment| segment.split(separator)).collect();
    }
    segments.into_iter().filter_map(command_word).collect()
}

fn spans(line: &str) -> impl Iterator<Item = &str> {
    line.split('`').skip(1).step_by(2)
}

pub fn scan(path: &str, text: &str) -> Vec<Citation> {
    let markdown = path.ends_with(".md");
    let mut fenced = false;
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if markdown && trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let commands = if !markdown {
            if trimmed.starts_with('#') { Vec::new() } else { in_command_position(line) }
        } else if fenced {
            in_command_position(line)
        } else {
            spans(line).flat_map(in_command_position).collect()
        };
        found.extend(commands.into_iter().map(|command| Citation { path: path.into(), line: index + 1, command: command.into() }));
    }
    found
}

fn tracked(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git").arg("-C").arg(root).args(["ls-files", "-z"]).output().map_err(|e| format!("git: {e}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files failed in {}", root.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| ROOTS.iter().any(|prefix| path.starts_with(prefix)) && !SKIPPED.iter().any(|skip| path.contains(skip)))
        .map(str::to_owned)
        .collect())
}

pub fn citations(root: &Path) -> Result<Vec<Citation>, String> {
    let mut found = Vec::new();
    for path in tracked(root)? {
        if let Ok(text) = std::fs::read_to_string(root.join(&path)) {
            found.extend(scan(&path, &text));
        }
    }
    Ok(found)
}

pub fn undefined(root: &Path, defined: fn(&str) -> bool) -> Result<Vec<Citation>, String> {
    Ok(citations(root)?.into_iter().filter(|citation| !defined(&citation.command)).collect())
}

pub fn verdict(root: &Path, defined: fn(&str) -> bool) -> Result<(), String> {
    let found = undefined(root, defined)?;
    if found.is_empty() {
        return Ok(());
    }
    let shown: Vec<_> = found.iter().take(5).map(|c| format!("{}:{} north {}", c.path, c.line, c.command)).collect();
    Err(format!("{} cited north subcommands are not defined: {}", found.len(), shown.join("; ")))
}

pub fn run(arguments: &[String], defined: fn(&str) -> bool) -> u8 {
    let (root, list) = match arguments {
        [] => (PathBuf::from("."), false),
        [flag] if flag == "--list" => (PathBuf::from("."), true),
        [flag, root] if flag == "--root" => (PathBuf::from(root), false),
        [flag, root, list] if flag == "--root" && list == "--list" => (PathBuf::from(root), true),
        _ => {
            eprintln!("Usage: north citation-check [--root DIR] [--list]");
            return 2;
        }
    };
    let found = if list { citations(&root) } else { undefined(&root, defined) };
    match found {
        Err(error) => {
            eprintln!("citation-check: {error}");
            1
        }
        Ok(found) => {
            for citation in &found {
                println!("{}:{}: north {}", citation.path, citation.line, citation.command);
            }
            u8::from(!list && !found.is_empty())
        }
    }
}

#[cfg(test)]
mod tests;
