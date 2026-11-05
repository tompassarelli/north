use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::candidate::git;
use super::slots::select::Config;

const SHOWN: usize = 8;

#[derive(Debug, PartialEq, Eq)]
pub struct Waiver<'a> {
    pub name: &'a str,
    pub reason: &'a str,
}

pub fn waivers(messages: &str) -> Vec<Waiver<'_>> {
    messages
        .lines()
        .filter_map(|line| line.strip_prefix("Removes-check:"))
        .filter_map(|rest| rest.trim().split_once(" because "))
        .map(|(name, reason)| Waiver { name: name.trim(), reason: reason.trim() })
        .filter(|waiver| !waiver.name.is_empty() && !waiver.name.contains(char::is_whitespace) && !waiver.reason.is_empty())
        .collect()
}

fn names(command: &str) -> impl Iterator<Item = &str> {
    command.split_whitespace().filter(|token| !token.starts_with('-') && !token.contains('=')).flat_map(|token| [token, token.rsplit('/').next().unwrap_or(token)]).filter(|name| !name.is_empty())
}

pub fn waives(name: &str, command: &str) -> bool {
    command == name || names(command).any(|candidate| candidate == name)
}

fn suggested<'a>(command: &'a str, others: &[&str]) -> &'a str {
    let shared = |name: &str| others.iter().any(|other| *other != command && names(other).any(|candidate| candidate == name));
    names(command).filter(|name| !name.contains('/')).find(|name| !shared(name)).or_else(|| names(command).last()).unwrap_or(command)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Uncovered {
    pub command: String,
    pub name: String,
    pub paths: Vec<String>,
}

pub fn uncovered(before: &BTreeMap<String, BTreeSet<String>>, after: &BTreeMap<String, BTreeSet<String>>) -> Vec<Uncovered> {
    let replacements: BTreeSet<&String> = after.iter().filter(|(command, _)| !before.contains_key(*command)).flat_map(|(_, paths)| paths).collect();
    let commands: Vec<&str> = before.keys().map(String::as_str).collect();
    before
        .iter()
        .filter_map(|(command, paths)| {
            let kept = after.get(command);
            let lost: Vec<String> = paths.iter().filter(|path| !kept.is_some_and(|kept| kept.contains(*path)) && !replacements.contains(path)).cloned().collect();
            (!lost.is_empty()).then(|| Uncovered { command: command.clone(), name: suggested(command, &commands).to_owned(), paths: lost })
        })
        .collect()
}

fn describe(gap: &Uncovered) -> String {
    let shown = gap.paths.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
    let more = gap.paths.len().saturating_sub(SHOWN);
    let tail = if more == 0 { String::new() } else { format!(" and {more} more") };
    format!("land check `{}` no longer covers {shown}{tail}; cover them with an equivalent check or add 'Removes-check: {} because REASON' to the commit message", gap.command, gap.name)
}

pub fn refuse_lost_coverage(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let fork = git(root, &["merge-base", base, head])?;
    let before = Config::load(root, &fork).coverage(root, head);
    if before.is_empty() {
        return Ok(());
    }
    let after = Config::load(root, head).coverage(root, head);
    let messages = git(root, &["log", "--format=%B", &format!("{fork}..{head}")])?;
    let waivers = waivers(&messages);
    let refusals: Vec<String> = uncovered(&before, &after).iter().filter(|gap| !waivers.iter().any(|waiver| waives(waiver.name, &gap.command))).map(describe).collect();
    if refusals.is_empty() { Ok(()) } else { Err(refusals.join("; ")) }
}
