use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::git;

const USAGE: &str = "usage: north land scan safe-push-verbs [REVISION]";
const SETTINGS: &str = ".safe-push";
const VERBS: &str = "src/cli/verbs.rs";
const COMMAND_KEYS: [&str; 6] = ["run", "check", "after", "warm", "build", "machine-bound"];
const TABLE_START: &str = "VerbKinds::new(&[";

static CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:^|[\s"'(=;|&])(?:[^\s"']*/)?(?:head-)?north[ \t]+([^\n]*)"#).expect("north call pattern"));

#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub reason: String,
}

pub fn verbs(table: &str) -> BTreeSet<String> {
    let rows = table.split_once(TABLE_START).map_or("", |(_, rest)| rest.split_once("]);").map_or(rest, |(rows, _)| rows));
    rows.lines().filter_map(|row| row.trim().strip_prefix("(\"")?.split('"').next()?.split_whitespace().next().map(str::to_owned)).collect()
}

fn command_text(line: &str) -> Option<&str> {
    let (key, value) = line.trim_start().split_once('=')?;
    let key = key.trim();
    match value.split_once(" :: ") {
        Some((_, command)) => COMMAND_KEYS.contains(&key).then_some(command),
        None => ["run", "build", "capacity", "warm"].contains(&key).then_some(value),
    }
}

fn word(token: &str) -> &str {
    token.trim_matches(|character| matches!(character, '"' | '\'' | '\\' | ';' | ')'))
}

pub fn findings(settings: &str, verbs: &BTreeSet<String>, exists: &dyn Fn(&str) -> bool) -> Vec<Finding> {
    let mut found = Vec::new();
    for (at, line) in settings.lines().enumerate() {
        let Some(command) = command_text(line) else { continue };
        for call in CALL.captures_iter(command) {
            let mut tokens = call[1].split_whitespace().map(word);
            let mut verb = None;
            while let Some(token) = tokens.next() {
                match token {
                    "--code" => {
                        for path in tokens.next().unwrap_or_default().split(',').filter(|path| !path.is_empty() && !path.starts_with('$')) {
                            if !exists(path) {
                                found.push(Finding { line: at + 1, reason: format!("--code names {path}, which the tree does not have") });
                            }
                        }
                    }
                    "--no-build" => {}
                    other => {
                        verb = Some(other);
                        break;
                    }
                }
            }
            if let Some(verb) = verb.filter(|verb| !verb.starts_with(['-', '$']) && !verb.is_empty() && !verbs.contains(*verb)) {
                found.push(Finding { line: at + 1, reason: format!("calls north {verb}, a verb {VERBS} does not list") });
            }
        }
    }
    found
}

pub fn run(directory: &Path, arguments: &[String]) -> u8 {
    let revision = match arguments {
        [] => "HEAD",
        [revision] if !revision.starts_with('-') => revision.as_str(),
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    let show = |path: &str| git::bytes(directory, &["show", &format!("{revision}:{path}")]).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let (Some(settings), Some(table)) = (show(SETTINGS), show(VERBS)) else { return 0 };
    let exists = |path: &str| git::text(directory, &["ls-tree", "--name-only", revision, "--", path]).is_some_and(|listing| !listing.trim().is_empty());
    let found = findings(&settings, &verbs(&table), &exists);
    for finding in &found {
        eprintln!("safe-push-verbs: {SETTINGS}:{}: {}", finding.line, finding.reason);
    }
    u8::from(!found.is_empty())
}

#[cfg(test)]
#[path = "safe_push_verbs_tests.rs"]
mod tests;
