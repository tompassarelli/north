use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;

use super::{gate_shadow, git};
use crate::platform::process;

const SELF: &str = "peer-invocation";
const REGISTRY: &str = "harness/gates.toml";
const MARKER: &str = "north ";
const PEER_MAIN: &str = "refs/remotes/origin/main";
const PEER_SOURCES: [&str; 3] = ["*.nix", "*.sh", "dotfiles/bin/"];
const OWN_SOURCES: [&str; 2] = ["src/", "crates/"];
const COMMAND_OPENERS: [char; 9] = ['"', '\'', '(', '|', '&', ';', '`', '{', '='];
const COMMAND_KEYWORDS: [&str; 4] = ["exec", "then", "do", "else"];
const HELP_LIMIT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    pub location: String,
    pub words: Vec<String>,
}

fn command_position(before: &str) -> bool {
    if before.ends_with('/') {
        return true;
    }
    let trimmed = before.trim_end();
    trimmed.is_empty() || trimmed.ends_with(COMMAND_OPENERS) || COMMAND_KEYWORDS.iter().any(|keyword| trimmed.split_whitespace().next_back() == Some(keyword))
}

pub fn invocations(location: &str, text: &str) -> Vec<Invocation> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        for (at, _) in line.match_indices(MARKER) {
            if !command_position(&line[..at]) {
                continue;
            }
            let mut words = Vec::new();
            for word in line[at + MARKER.len()..].split_whitespace() {
                let closes = word.ends_with(['"', '\'', ';', '`', ')']);
                let word = word.trim_end_matches(['"', '\'', ';', '`', ')']);
                if word.starts_with(['&', '|', '>', '<']) {
                    break;
                }
                if !word.is_empty() {
                    words.push(word.to_owned());
                }
                if closes {
                    break;
                }
            }
            if !words.is_empty() {
                found.push(Invocation { location: format!("{location}:{}", index + 1), words });
            }
        }
    }
    found
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Dropped {
    pub flags: BTreeSet<String>,
    pub names: BTreeSet<String>,
}

pub fn dropped(diff: &str) -> Dropped {
    let flag = Regex::new(r"--[a-z][a-z0-9-]*").unwrap();
    let name = Regex::new(r#""([a-z][a-z0-9-]*)""#).unwrap();
    let (mut flags, mut names): (BTreeMap<String, i64>, BTreeMap<String, i64>) = (BTreeMap::new(), BTreeMap::new());
    for line in diff.lines().filter(|line| !line.starts_with("---") && !line.starts_with("+++")) {
        let weight = match line.chars().next() {
            Some('-') => 1,
            Some('+') => -1,
            _ => continue,
        };
        for found in flag.find_iter(line) {
            *flags.entry(found.as_str().to_owned()).or_default() += weight;
        }
        for found in name.captures_iter(line) {
            *names.entry(found[1].to_owned()).or_default() += weight;
        }
    }
    let net = |counts: BTreeMap<String, i64>| counts.into_iter().filter(|(_, count)| *count > 0).map(|(word, _)| word).collect();
    Dropped { flags: net(flags), names: net(names) }
}

fn names_word(help: &str, word: &str) -> bool {
    help.match_indices(word).any(|(at, _)| {
        let before = help[..at].chars().next_back();
        let after = help[at + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '-') && !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
    })
}

fn is_name(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_lowercase()) && word.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn problems(invocation: &Invocation, dropped: &Dropped, ask: &mut dyn FnMut(&[&str]) -> Option<String>) -> Vec<String> {
    let leading: Vec<&str> = invocation.words.iter().map(String::as_str).take_while(|word| is_name(word)).collect();
    let flags: Vec<&str> = invocation.words.iter().map(String::as_str).take_while(|word| *word != "--").filter_map(|word| word.split('=').next().filter(|flag| flag.starts_with("--"))).filter(|flag| dropped.flags.contains(*flag)).collect();
    let unknown_name = leading.iter().any(|word| dropped.names.contains(*word));
    if leading.is_empty() || (flags.is_empty() && !unknown_name) {
        return Vec::new();
    }
    let answered = (1..=leading.len()).rev().find_map(|length| ask(&leading[..length]).map(|help| (length, help)));
    let Some((length, help)) = answered else {
        return if dropped.names.contains(leading[0]) { vec![format!("{}: runs `north {}`, a subcommand this lane drops", invocation.location, leading[0])] } else { Vec::new() };
    };
    let command = leading[..length].join(" ");
    let mut found = Vec::new();
    if let Some(next) = leading.get(length).filter(|next| dropped.names.contains(**next) && !names_word(&help, next)) {
        found.push(format!("{}: runs `north {command} {next}`, a subcommand this lane drops", invocation.location));
    }
    found.extend(flags.into_iter().filter(|flag| !names_word(&help, flag)).map(|flag| format!("{}: runs `north {command}` with {flag}, which this lane drops from `north {command} --help`", invocation.location)));
    found
}

fn peer_invocations(peer: &Path) -> Option<Vec<Invocation>> {
    let mut arguments = vec!["grep", "-z", "-n", "-I", "-F", "-e", MARKER, PEER_MAIN, "--"];
    arguments.extend(PEER_SOURCES);
    let output = crate::platform::git::run(peer, &arguments).ok()?;
    match output.status.code() {
        Some(0) => {}
        Some(1) if output.stdout.is_empty() => return Some(Vec::new()),
        _ => return None,
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("{PEER_MAIN}:");
    let mut found = Vec::new();
    for record in text.split('\n').filter(|record| !record.is_empty()) {
        let mut fields = record.splitn(3, '\0');
        let (Some(path), Some(line), Some(body)) = (fields.next(), fields.next(), fields.next()) else { continue };
        let path = path.strip_prefix(&prefix).unwrap_or(path);
        let Ok(line) = line.parse::<usize>() else { continue };
        found.extend(invocations(path, body).into_iter().map(|invocation| Invocation { location: format!("{path}:{line}"), words: invocation.words }));
    }
    Some(found)
}

fn enforcing(directory: &Path, revisions: [&str; 2]) -> bool {
    revisions.iter().any(|revision| git::text(directory, &["show", &format!("{revision}:{REGISTRY}")]).is_some_and(|text| gate_shadow::gates(&text).get(SELF).is_some_and(|gate| gate.enforcing)))
}

fn record(state: &Path, head: &str, refused: &[String]) {
    let directory = state.join("north/gates");
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = serde_json::json!({"gate": SELF, "at": at, "head": head, "would_refuse": !refused.is_empty(), "refused": refused});
    let _ = std::fs::create_dir_all(&directory);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(directory.join(format!("{SELF}.jsonl"))) {
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

pub fn own_help(path: &[&str]) -> Option<String> {
    let mut command = Command::new(std::env::current_exe().ok()?);
    command.args(path).arg("--help");
    let output = process::run_group_within(command, HELP_LIMIT)?;
    output.status.success().then(|| format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
}

pub fn scan(directory: &Path, from: &str, head: &str, peer: &Path, state: &Path, ask: &mut dyn FnMut(&[&str]) -> Option<String>, out: &mut dyn Write) -> bool {
    let mut arguments = vec!["diff", "-U0", "--no-color", "--no-ext-diff", from, head, "--"];
    arguments.extend(OWN_SOURCES);
    let dropped = dropped(&git::text(directory, &arguments).unwrap_or_default());
    if dropped.flags.is_empty() && dropped.names.is_empty() {
        return false;
    }
    let Some(peers) = peer_invocations(peer) else {
        let _ = writeln!(out, "{SELF}: ⚠ no {} with {PEER_MAIN}, so the flags and subcommands this lane drops were not checked against its units and scripts", peer.display());
        return false;
    };
    let refused: Vec<String> = peers.iter().flat_map(|invocation| problems(invocation, &dropped, ask)).collect();
    let enforce = enforcing(directory, [from, head]);
    if !enforce {
        record(state, head, &refused);
    }
    if refused.is_empty() {
        return false;
    }
    let verb = if enforce { "refused" } else { "would refuse (shadow)" };
    let _ = writeln!(out, "safe-push: ✗ {SELF} {verb}: this lane drops a north flag or subcommand that {} still runs, so its units crash-loop on their next restart; keep it or land the nixos-config change first:", peer.display());
    for line in &refused {
        let _ = writeln!(out, "  {line}");
    }
    enforce
}

#[cfg(test)]
#[path = "peer_invocations_tests.rs"]
mod tests;
