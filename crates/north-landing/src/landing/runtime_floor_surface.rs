use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::invocation::Invocation;
use crate::platform::git as platform_git;

const COMMAND_TABLE: [&str; 4] = ["src/main.rs", "src/cli/mod.rs", "src/cli/tools.rs", "src/cli/session.rs"];
const UNIVERSAL: [&str; 2] = ["--help", "--version"];
const SEED_LIMIT: usize = 60;

static LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([a-z][a-z0-9-]*)""#).expect("literal pattern"));
static FLAG_LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""(--[a-z][a-z0-9-]*)["=]"#).expect("flag literal pattern"));
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([a-z][a-z0-9_]*)::|\bmod\s+([a-z][a-z0-9_]*)\s*;").expect("reference pattern"));

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Gap {
    Command(String),
    Flag { command: String, flag: String },
}

impl Gap {
    pub fn is_flag(&self) -> bool {
        matches!(self, Self::Flag { .. })
    }

    pub fn name(&self, invocation: &Invocation) -> String {
        match self {
            Self::Command(command) => format!("north {command}"),
            Self::Flag { flag, .. } => format!("north {} {flag}", invocation.path()),
        }
    }
}

#[derive(Debug, Default)]
pub struct Surface {
    commands: BTreeSet<String>,
    files: BTreeSet<String>,
    flags: BTreeSet<(String, String)>,
    scopes: BTreeMap<String, BTreeSet<String>>,
}

fn git_text(repo: &Path, args: &[&str]) -> Option<String> {
    platform_git::stdout(repo, args, |output| String::from_utf8_lossy(&output.stderr).into_owned()).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn is_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("tests/") || path.contains("/tests/") || path.contains("/fixtures/") || name == "tests.rs" || name.ends_with("_tests.rs")
}

fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".rs").unwrap_or(name)
}

fn in_module(path: &str, word: &str) -> bool {
    let wanted = word.replace('-', "_");
    path.split('/').any(|part| {
        let part = part.strip_suffix(".rs").unwrap_or(part);
        part == wanted || part.strip_prefix(&wanted).is_some_and(|rest| rest.starts_with('_'))
    })
}

pub fn command_words(tables: &[String]) -> BTreeSet<String> {
    tables.iter().flat_map(|source| LITERAL.captures_iter(source).map(|capture| capture[1].to_owned())).collect()
}

pub fn flag_hits(grepped: &str, rev: &str) -> BTreeSet<(String, String)> {
    let prefix = format!("{rev}:");
    grepped
        .lines()
        .filter_map(|line| line.split_once('\0'))
        .filter_map(|(path, hit)| {
            let path = path.strip_prefix(&prefix).unwrap_or(path);
            let flag = FLAG_LITERAL.captures(hit)?;
            (!is_test_path(path)).then(|| (path.to_owned(), flag[1].to_owned()))
        })
        .collect()
}

pub fn references(source: &str) -> BTreeSet<String> {
    REFERENCE.captures_iter(source).filter_map(|capture| capture.get(1).or_else(|| capture.get(2))).map(|name| name.as_str().to_owned()).collect()
}

pub fn reach(seeds: &BTreeSet<String>, files: &BTreeSet<String>, referenced: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    let nested: BTreeSet<&str> = seeds.iter().filter_map(|path| path.rsplit_once('/')).map(|(dir, _)| dir).filter(|dir| !dir.ends_with("/src") && *dir != "src").collect();
    let names: BTreeSet<&String> = seeds.iter().filter_map(|path| referenced.get(path)).flatten().collect();
    files
        .iter()
        .filter(|path| seeds.contains(*path) || nested.iter().any(|dir| path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))) || names.iter().any(|name| name.as_str() == stem(path)))
        .cloned()
        .collect()
}

impl Surface {
    pub fn read(repo: &Path, rev: &str, words: &BTreeSet<String>) -> Self {
        let tables: Vec<String> = COMMAND_TABLE.iter().filter_map(|path| git_text(repo, &["show", &format!("{rev}:{path}")])).collect();
        let files: BTreeSet<String> = git_text(repo, &["ls-tree", "-r", "--name-only", "-z", rev])
            .map(|listing| listing.split('\0').filter(|path| path.ends_with(".rs") && !is_test_path(path)).map(str::to_owned).collect())
            .unwrap_or_default();
        let flags = git_text(repo, &["grep", "--null", "-I", "-o", "-E", r#""--[a-z][a-z0-9-]*["=]"#, rev, "--", "*.rs"]).map(|grepped| flag_hits(&grepped, rev)).unwrap_or_default();
        let scopes = words.iter().filter_map(|word| Some((word.clone(), Self::scope(repo, rev, word, &files)?))).collect();
        Self { commands: command_words(&tables), files, flags, scopes }
    }

    fn scope(repo: &Path, rev: &str, word: &str, files: &BTreeSet<String>) -> Option<BTreeSet<String>> {
        let prefix = format!("{rev}:");
        let listed = git_text(repo, &["grep", "-l", "-I", "-F", "-e", &format!("\"{word}\""), rev, "--", "*.rs"])?;
        let seeds: BTreeSet<String> = listed
            .lines()
            .map(|line| line.strip_prefix(&prefix).unwrap_or(line))
            .filter(|path| files.contains(*path) && !COMMAND_TABLE.contains(path))
            .map(str::to_owned)
            .collect();
        if seeds.is_empty() || seeds.len() > SEED_LIMIT {
            return None;
        }
        let referenced: BTreeMap<String, BTreeSet<String>> =
            seeds.iter().filter_map(|path| Some((path.clone(), references(&git_text(repo, &["show", &format!("{rev}:{path}")])?)))).collect();
        Some(reach(&seeds, files, &referenced))
    }

    pub fn from_parts(commands: &[&str], files: &[&str], flags: &[(&str, &str)]) -> Self {
        Self {
            commands: commands.iter().map(|command| (*command).to_owned()).collect(),
            files: files.iter().map(|file| (*file).to_owned()).collect(),
            flags: flags.iter().map(|(path, flag)| ((*path).to_owned(), (*flag).to_owned())).collect(),
            scopes: BTreeMap::new(),
        }
    }

    pub fn commands(&self) -> &BTreeSet<String> {
        &self.commands
    }

    fn owners(&self, invocation: &Invocation) -> BTreeSet<&str> {
        let named = invocation.command().into_iter().flat_map(|command| self.files.iter().filter(move |path| in_module(path, command)));
        let reached = invocation.words.iter().rev().find_map(|word| self.scopes.get(word)).into_iter().flatten();
        named.chain(reached).map(String::as_str).collect()
    }

    pub fn gaps(&self, invocation: &Invocation) -> Vec<Gap> {
        let Some(command) = invocation.command() else { return Vec::new() };
        if !self.commands.contains(command) {
            return vec![Gap::Command(command.to_owned())];
        }
        let owners = self.owners(invocation);
        if owners.is_empty() {
            return Vec::new();
        }
        let accepted = |flag: &String| self.flags.iter().any(|(path, known)| known == flag && owners.contains(path.as_str()));
        invocation
            .flags
            .iter()
            .filter(|flag| !UNIVERSAL.contains(&flag.as_str()) && !accepted(flag))
            .map(|flag| Gap::Flag { command: command.to_owned(), flag: flag.clone() })
            .collect()
    }
}
