use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

const COMMAND_TABLES: &str = "src/cli";
const FLAG_SOURCES: [&str; 6] = ["src", "crates", ":(exclude)*_tests.rs", ":(exclude)*/tests/*", ":(exclude)tests", ":(exclude)*.md"];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Bin,
    Command,
    Flag,
}

#[derive(Debug, Default)]
pub struct Removed {
    pub bins: Vec<String>,
    pub commands: Vec<String>,
    pub flags: Vec<String>,
}

impl Removed {
    pub fn is_empty(&self) -> bool {
        self.bins.is_empty() && self.commands.is_empty() && self.flags.is_empty()
    }
}

fn grep_literals(repo: &Path, rev: &str, pattern: &str, pathspecs: &[&str]) -> Result<BTreeSet<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["grep", "-h", "-o", "-I", "-E", "-e", pattern, rev, "--"])
        .args(pathspecs)
        .output()
        .map_err(|error| format!("git grep: {error}"))?;
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(BTreeSet::new()),
        _ => return Err(format!("git grep at {rev} failed: {}", String::from_utf8_lossy(&output.stderr).trim())),
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.split('"').nth(1)).map(str::to_owned).collect())
}

fn commands(repo: &Path, rev: &str) -> Result<BTreeSet<String>, String> {
    grep_literals(repo, rev, "^[[:space:]]+\\(\"[a-z][a-z0-9-]*\",", &[COMMAND_TABLES])
}

fn flags(repo: &Path, rev: &str) -> Result<BTreeSet<String>, String> {
    grep_literals(repo, rev, "\"--[a-z0-9][a-z0-9-]*\"", &FLAG_SOURCES)
}

pub fn removed_commands_and_flags(repo: &Path, base: &str, head: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let commands = commands(repo, base)?.difference(&commands(repo, head)?).cloned().collect();
    let flags = flags(repo, base)?.difference(&flags(repo, head)?).cloned().collect();
    Ok((commands, flags))
}

pub fn invocation(line: &str, prefixes: &[&str]) -> Option<Vec<String>> {
    let rest = prefixes.iter().find_map(|prefix| {
        let needle = format!("{prefix}north ");
        line.find(&needle).map(|at| &line[at + needle.len()..])
    })?;
    let mut words = Vec::new();
    for word in rest.split_whitespace() {
        let closes = word.ends_with(['"', ';']);
        let word = word.trim_end_matches(['"', ';']);
        if !word.is_empty() {
            words.push(word.to_owned());
        }
        if closes {
            break;
        }
    }
    Some(words)
}

pub fn removed_in(words: &[String], removed: &Removed) -> Vec<(Kind, String)> {
    let mut found = Vec::new();
    if let Some(command) = words.first().filter(|word| !word.starts_with('-'))
        && removed.commands.contains(command)
    {
        found.push((Kind::Command, command.clone()));
    }
    for word in words.iter().filter(|word| word.starts_with("--")) {
        let flag = word.split('=').next().unwrap_or(word);
        if removed.flags.iter().any(|name| name == flag) {
            found.push((Kind::Flag, flag.to_owned()));
        }
    }
    found
}
