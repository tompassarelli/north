use regex::Regex;
use std::sync::LazyLock;

use super::cleanup;

static TICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t>]*[-*][ \t]+\[[xX]\][ \t]+(.*)$").expect("ticked box pattern"));
static PASSED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Evidence: `[^`]+` (?:exited 0|passed) on ").expect("oracle evidence pattern"));
static STAMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Evidence: `(?P<command>[^`]+)` (?:(?:passed|exited 0) on main )[^,]+?(?:, exit (?P<exit>[0-9]+))?\.").expect("checker stamp pattern"));
static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bsplit\b[^.;]*?(?:[A-Za-z0-9_.-]+/)?[A-Za-z0-9_.-]+#\d+").expect("split owner pattern"));
static LANDED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*landed on origin/main\b").expect("landed box pattern"));

pub fn unproven(body: &str) -> Vec<&str> {
    body.lines()
        .filter(|line| !cleanup::is_box(line))
        .filter_map(|line| TICKED.captures(line)?.get(1).map(|text| text.as_str().trim()))
        .filter(|text| !LANDED.is_match(text) && !passed(text) && !SPLIT.is_match(text))
        .collect()
}

pub fn evidence(command: &str, exit: i32, sha: &str, date: &str) -> String {
    format!("Evidence: `{command}` passed on main {} on {date}, exit {exit}.", &sha[..sha.len().min(12)])
}

/// The command a box's own `Done when:` names, when it names one.
pub fn done_when_command(text: &str) -> Option<&str> {
    let (_, after) = text.split_once("Done when: `")?;
    let (command, _) = after.split_once('`')?;
    let command = command.trim();
    (!command.is_empty()).then_some(command)
}

/// Whether a ticked box carries the checker's stamp for its own `Done when:` command: the evidence names that
/// command and it exited 0 on main, so a box another command's stamp names is not counted as this box verified.
pub fn own_command_passed(text: &str) -> bool {
    let Some(command) = done_when_command(text) else { return false };
    STAMP.captures(text).is_some_and(|found| found["command"].trim() == command && found.name("exit").is_none_or(|exit| exit.as_str() == "0"))
}

pub fn passed(text: &str) -> bool {
    PASSED.is_match(text)
}

#[cfg(test)]
mod tests;
