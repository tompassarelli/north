use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

const COMMAND_TABLES: &str = "src/cli";
const COMMAND_DEPTH: usize = 2;
const VERB_KINDS: &str = "src/cli/verbs.rs";
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
    pub live: Vec<String>,
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

fn live_paths(repo: &Path, rev: &str) -> Result<BTreeSet<String>, String> {
    grep_literals(repo, rev, "^[[:space:]]+\\(\"[a-z][a-z0-9-]* [a-z][a-z0-9-]*\",", &[VERB_KINDS])
}

fn flags(repo: &Path, rev: &str) -> Result<BTreeSet<String>, String> {
    grep_literals(repo, rev, "\"--[a-z0-9][a-z0-9-]*\"", &FLAG_SOURCES)
}

pub fn removed_surface(repo: &Path, base: &str, head: &str, bins: Vec<String>) -> Result<Removed, String> {
    let commands = commands(repo, base)?.difference(&commands(repo, head)?).cloned().collect();
    let flags = flags(repo, base)?.difference(&flags(repo, head)?).cloned().collect();
    Ok(Removed { bins, commands, flags, live: live_paths(repo, head)?.into_iter().collect() })
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
    let live = |depth: usize, word: &str| depth > 0 && removed.live.iter().any(|path| *path == format!("{} {word}", words[0]));
    for (depth, command) in words.iter().take_while(|word| !word.starts_with('-')).take(COMMAND_DEPTH).enumerate() {
        if removed.commands.contains(command) && !live(depth, command) {
            found.push((Kind::Command, command.clone()));
        }
    }
    for word in words.iter().filter(|word| word.starts_with("--")) {
        let flag = word.split('=').next().unwrap_or(word);
        if removed.flags.iter().any(|name| name == flag) {
            found.push((Kind::Flag, flag.to_owned()));
        }
    }
    found
}

#[cfg(test)]
#[path = "removed_surface_tests.rs"]
mod tests;
