use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOTS: &[&str] = &["docs/", "harness/", "bin/", ".claude/", "agent-machinery/", "README.md", "AGENTS.md"];
const SKIPPED: &[&str] = &["docs/rationale/", "/fixtures/", ".test.", "node_modules/"];
const SEPARATORS: &[&str] = &["&&", "||", "$(", ";", "|", "(", "`"];
const NAMED_PATHS: &[&str] = &["docs/", "README.md", "AGENTS.md"];
const LINKS_ONLY: &[&str] = &["docs/references/"];
const UNSCANNED: &[&str] = &["docs/features/"];
const SKILL_ROOTS: &[&str] = &["harness/skills/", "agent-machinery/skills/"];

#[derive(Debug, Eq, PartialEq)]
pub struct Citation {
    pub path: String,
    pub line: usize,
    pub command: String,
    pub flags: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct MissingFlag {
    pub path: String,
    pub line: usize,
    pub command: String,
    pub flag: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct BrokenPath {
    pub path: String,
    pub line: usize,
    pub target: String,
}

fn flag(token: &str) -> Option<&str> {
    let name = token.strip_prefix("--")?.split('=').next()?;
    let name = name.trim_end_matches(['.', ',', ':', '\'', '"']);
    (name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .then(|| &token[..name.len() + 2])
}

fn command_word(segment: &str) -> Option<(&str, Vec<&str>)> {
    let segment = segment.trim_start();
    let segment = segment.strip_prefix("$ ").unwrap_or(segment);
    let segment = segment.strip_prefix("exec ").unwrap_or(segment);
    let rest = segment.strip_prefix("north ")?.trim_start();
    let word = rest.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')).next()?;
    let flags = rest[word.len()..].split_whitespace().filter_map(flag).collect();
    word.starts_with(|c: char| c.is_ascii_lowercase()).then_some((word, flags))
}

fn in_command_position(text: &str) -> Vec<(&str, Vec<&str>)> {
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
        found.extend(commands.into_iter().map(|(command, flags)| Citation {
            path: path.into(),
            line: index + 1,
            command: command.into(),
            flags: flags.into_iter().map(str::to_owned).collect(),
        }));
    }
    found
}

fn all_tracked(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git").arg("-C").arg(root).args(["ls-files", "-z"]).output().map_err(|e| format!("git: {e}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files failed in {}", root.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

fn tracked(root: &Path) -> Result<Vec<String>, String> {
    Ok(all_tracked(root)?
        .into_iter()
        .filter(|path| ROOTS.iter().any(|prefix| path.starts_with(prefix)) && !SKIPPED.iter().any(|skip| path.contains(skip)))
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

fn flag_literals(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut literals = BTreeSet::new();
    for path in all_tracked(root)? {
        if !(path.starts_with("src/") || path.starts_with("crates/")) || !path.ends_with(".rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else { continue };
        for (at, _) in text.match_indices("\"--") {
            let rest = &text[at + 1..];
            if let Some(end) = rest[2..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                && rest[2 + end..].starts_with(['"', '='])
            {
                literals.insert(rest[..2 + end].to_owned());
            }
        }
    }
    Ok(literals)
}

pub fn missing_flags(root: &Path, defined: fn(&str) -> bool) -> Result<Vec<MissingFlag>, String> {
    let literals = flag_literals(root)?;
    Ok(citations(root)?
        .into_iter()
        .filter(|citation| defined(&citation.command))
        .flat_map(|citation| {
            let Citation { path, line, command, flags } = citation;
            flags.into_iter().filter(|flag| !literals.contains(flag)).map(move |flag| MissingFlag { path: path.clone(), line, command: command.clone(), flag })
        })
        .collect())
}

fn link_targets(line: &str) -> Vec<&str> {
    line.match_indices("](")
        .filter_map(|(at, _)| {
            let rest = &line[at + 2..];
            let end = rest.find(|c: char| c == ')' || c.is_whitespace())?;
            rest[end..].starts_with(')').then(|| &rest[..end])
        })
        .filter(|target| {
            let scheme = target.split_once(':').is_some_and(|(head, _)| !head.is_empty() && head.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)));
            !(target.is_empty() || scheme || target.starts_with(['#', '/', '<']))
        })
        .collect()
}

fn placeholder(target: &str) -> bool {
    target.contains(['<', '>', '*', '{', '$', '~', '[']) || target.contains("...")
        || target.split('/').any(|segment| {
            segment.chars().any(|c| c.is_ascii_uppercase())
                && segment.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '-')
        })
}

fn named_path<'a>(span: &'a str, tops: &BTreeSet<String>) -> Option<&'a str> {
    let token = span.trim_end_matches(['.', ',', ';', ')']);
    let token = token.split('#').next()?;
    let token = match token.rsplit_once(':') {
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit() || c == '-') => head,
        _ => token,
    };
    let first = token.trim_start_matches("../").split('/').next()?;
    (token.contains('/') && !token.contains(char::is_whitespace) && tops.contains(first) && !placeholder(token)).then_some(token)
}

fn normalized(dir: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn resolves(known: &BTreeSet<String>, dir: &str, target: &str) -> bool {
    let target = target.split('#').next().unwrap_or("");
    let Some(path) = normalized(dir, target) else { return true };
    if known.contains(&path) {
        return true;
    }
    SKILL_ROOTS.iter().any(|from| {
        path.strip_prefix(from)
            .is_some_and(|rest| SKILL_ROOTS.iter().any(|to| to != from && known.contains(&format!("{to}{rest}"))))
    })
}

pub fn scan_paths(path: &str, text: &str, known: &BTreeSet<String>, tops: &BTreeSet<String>) -> Vec<BrokenPath> {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let names = NAMED_PATHS.iter().any(|prefix| path.starts_with(prefix)) && !LINKS_ONLY.iter().any(|prefix| path.starts_with(prefix));
    let mut fenced = false;
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut targets: Vec<&str> = link_targets(line).into_iter().filter(|target| !resolves(known, dir, target)).collect();
        if names {
            targets.extend(spans(line).filter_map(|span| named_path(span, tops)).filter(|target| !resolves(known, "", target) && !resolves(known, dir, target)));
        }
        found.extend(targets.into_iter().map(|target| BrokenPath { path: path.into(), line: index + 1, target: target.into() }));
    }
    found
}

fn known_paths(files: &[String]) -> BTreeSet<String> {
    let mut known = BTreeSet::new();
    for file in files {
        let mut prefix = String::new();
        for part in file.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            known.insert(prefix.clone());
        }
    }
    known
}

pub fn broken_paths(root: &Path) -> Result<Vec<BrokenPath>, String> {
    let files = all_tracked(root)?;
    let known = known_paths(&files);
    let tops: BTreeSet<String> = files.iter().filter_map(|file| file.split_once('/')).map(|(top, _)| top.to_owned()).collect();
    let mut found = Vec::new();
    for path in tracked(root)? {
        if !path.ends_with(".md") || UNSCANNED.iter().any(|prefix| path.starts_with(prefix)) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(root.join(&path)) {
            found.extend(scan_paths(&path, &text, &known, &tops));
        }
    }
    Ok(found)
}

pub fn verdict(root: &Path, defined: fn(&str) -> bool) -> Result<(), String> {
    let found = undefined(root, defined)?;
    let flags = missing_flags(root, defined)?;
    let broken = broken_paths(root)?;
    if found.is_empty() && flags.is_empty() && broken.is_empty() {
        return Ok(());
    }
    let mut problems = Vec::new();
    if !found.is_empty() {
        let shown: Vec<_> = found.iter().take(5).map(|c| format!("{}:{} north {}", c.path, c.line, c.command)).collect();
        problems.push(format!("{} cited north subcommands are not defined: {}", found.len(), shown.join("; ")));
    }
    if !flags.is_empty() {
        let shown: Vec<_> = flags.iter().take(5).map(|f| format!("{}:{} north {} {}", f.path, f.line, f.command, f.flag)).collect();
        problems.push(format!("{} cited flags occur in no source string literal: {}", flags.len(), shown.join("; ")));
    }
    if !broken.is_empty() {
        let shown: Vec<_> = broken.iter().take(5).map(|b| format!("{}:{} {}", b.path, b.line, b.target)).collect();
        problems.push(format!("{} named paths do not exist: {}", broken.len(), shown.join("; ")));
    }
    Err(problems.join("; "))
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
    let flags = if list { Ok(Vec::new()) } else { missing_flags(&root, defined) };
    let broken = if list { Ok(Vec::new()) } else { broken_paths(&root) };
    match (found, flags, broken) {
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
            eprintln!("citation-check: {error}");
            1
        }
        (Ok(found), Ok(flags), Ok(broken)) => {
            for citation in &found {
                println!("{}:{}: north {}", citation.path, citation.line, citation.command);
            }
            for flag in &flags {
                println!("{}:{}: north {} cites missing flag {}", flag.path, flag.line, flag.command, flag.flag);
            }
            for path in &broken {
                println!("{}:{}: missing path {}", path.path, path.line, path.target);
            }
            u8::from(!list && (!found.is_empty() || !flags.is_empty() || !broken.is_empty()))
        }
    }
}

#[cfg(test)]
mod tests;
