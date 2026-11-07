//! The dispatcher answers `--help` for every verb before any verb runs, so a probe such as firn's
//! unit-flags check never starts a sampler, a post or a launch: from the contract a verb publishes
//! when it has one, else from its reference.txt lines.
use super::CLI_REFERENCE;
use crate::capacity::WATCHDOG_USAGE;
use crate::land::serve;
use north_safe_push::entry;
use crate::lanes::GC_USAGE;

const ASKS: [&str; 2] = ["--help", "-h"];
const WRAPPERS: [&[&str]; 8] = [
    &["capacity", "gate"],
    &["gh-session"],
    &["git-credential"],
    &["resident-cap", "gate"],
    &["session-unit"],
    &["ssh-argv"],
    &["ssh-session"],
    &["threads"],
];
const CONTRACTS: [(&[&str], &str); 6] = [
    (&["capacity", "watchdog"], WATCHDOG_USAGE),
    (&["cloud"], super::cloud::USAGE),
    (&["fleet"], super::fleet::usage::USAGE),
    (&["land", "serve"], serve::USAGE),
    (&["lane", "gc"], GC_USAGE),
    (&["safe-push"], entry::USAGE),
];

fn starts_with(arguments: &[String], prefix: &[&str]) -> bool {
    arguments.len() >= prefix.len() && arguments.iter().zip(prefix).all(|(word, expected)| word == expected)
}

pub(super) fn requested(arguments: &[String]) -> bool {
    let asks = |word: &String| ASKS.contains(&word.as_str());
    if let Some(wrapper) = WRAPPERS.iter().find(|wrapper| starts_with(arguments, wrapper)) {
        return matches!(&arguments[wrapper.len()..], [word] if asks(word));
    }
    arguments.iter().skip(1).take_while(|word| *word != "--").any(asks)
}

pub(super) fn path(arguments: &[String]) -> Vec<String> {
    arguments.iter().take_while(|word| !word.starts_with('-')).cloned().collect()
}

struct Entry<'a> {
    words: Vec<&'a str>,
    lines: Vec<&'a str>,
}

fn entries() -> Vec<Entry<'static>> {
    entries_of(CLI_REFERENCE)
}

fn entries_of(reference: &str) -> Vec<Entry<'_>> {
    let commands = reference.split_once("Commands:\n").map_or("", |(_, rest)| rest);
    let commands = commands.split_once("\nOptions:").map_or(commands, |(listed, _)| listed);
    let mut entries: Vec<Entry> = Vec::new();
    for line in commands.lines().filter(|line| !line.trim().is_empty()) {
        let continues = line.starts_with("   ");
        match entries.last_mut() {
            Some(entry) if continues => entry.lines.push(line),
            _ => entries.push(Entry { words: line.split_whitespace().collect(), lines: vec![line] }),
        }
    }
    entries
}

fn names(token: &str, word: &str) -> bool {
    token.trim_matches(['[', ']', '(', ')']).split('|').any(|alternative| alternative == word)
}

fn covers(entry: &Entry, path: &[String]) -> bool {
    path.len() <= entry.words.len() && path.iter().zip(&entry.words).all(|(word, token)| names(token, word))
}

pub(super) fn usage(path: &[String]) -> Option<String> {
    let entries = entries();
    let literal = |length: usize| entries.iter().any(|entry| entry.words.len() >= path.len() && path.iter().zip(&entry.words).all(|(word, token)| word == token) && path.len() > length);
    if let Some((prefix, contract)) = CONTRACTS.iter().filter(|(prefix, _)| starts_with(path, prefix)).max_by_key(|(prefix, _)| prefix.len())
        && !literal(prefix.len())
    {
        return Some(format!("{}\n", contract.trim_end()));
    }
    let matched = (1..=path.len()).rev().map(|length| &path[..length]).find_map(|prefix| {
        let listed: Vec<&Entry> = entries.iter().filter(|entry| covers(entry, prefix)).collect();
        (!listed.is_empty()).then_some(listed)
    })?;
    let mut text = String::from("Usage:\n");
    for line in matched.iter().flat_map(|entry| &entry.lines) {
        match line.strip_prefix("  ").filter(|rest| !rest.starts_with(' ')) {
            Some(rest) => text.push_str(&format!("  north {rest}\n")),
            None => text.push_str(&format!("{line}\n")),
        }
    }
    Some(text)
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Resolution {
    Command,
    Group,
    Unknown(String),
    NoVerb(String),
}

fn literal(token: &str) -> bool {
    !token.starts_with(['-', '[', '(', '<']) && !token.contains("...") && !token.chars().any(|c| c.is_ascii_uppercase())
}

fn literals<'a>(entry: &Entry<'a>) -> Vec<&'a str> {
    let usage = entry.lines[0].trim_start();
    let usage = usage.split_once("  ").map_or(usage, |(usage, _)| usage);
    usage.split_whitespace().take_while(|token| literal(token)).collect()
}

fn agrees(literals: &[&str], path: &[String]) -> bool {
    literals.iter().zip(path).all(|(token, word)| names(token, word))
}

/// How the command table in `reference` reads the words of one north call: a whole command, a group whose subcommand is
/// appended at run time, or a path it lacks, with what it lists where the call parts from it.
fn tables<'a>(reference: &'a str, verb: &str) -> Vec<Vec<&'a str>> {
    entries_of(reference).iter().map(literals).filter(|words| words.first().is_some_and(|first| names(first, verb))).collect()
}

fn listing(reference: &str, path: &[String]) -> Result<Resolution, String> {
    let tables = tables(reference, &path[0]);
    if tables.is_empty() {
        return Ok(Resolution::Command);
    }
    for depth in 1..=path.len() {
        let under: Vec<&Vec<&str>> = tables.iter().filter(|words| words.len() >= depth && agrees(&words[..depth], &path[..depth])).collect();
        let whole = under.iter().any(|words| words.len() == depth);
        if depth == path.len() {
            return Ok(if whole { Resolution::Command } else { Resolution::Group });
        }
        if under.iter().any(|words| words.len() > depth && names(words[depth], &path[depth])) {
            continue;
        }
        if whole {
            return Ok(Resolution::Command);
        }
        let mut listed: Vec<&str> = under.iter().filter(|words| words.len() > depth).map(|words| words[depth]).collect();
        listed.dedup();
        return Err(format!("this runtime has no north {}; north {} takes {}", path[..=depth].join(" "), path[..depth].join(" "), listed.join(", ")));
    }
    Ok(Resolution::Command)
}

/// How this runtime reads one north call. Its dispatcher answers for the verb; below the verb, the command table is not
/// closed (a verb may dispatch subcommands it does not list), so a path counts as dropped only when the `live` runtime's
/// table lists it and `reference` no longer does.
pub(super) fn resolve(reference: &str, live: &str, path: &[String], defined: impl Fn(&str) -> bool) -> Resolution {
    let Some(verb) = path.first() else { return Resolution::Command };
    if !defined(verb) {
        let hint = super::moved::hint(path).map_or_else(String::new, |hint| format!("; {hint}"));
        let reason = format!("this runtime has no north {verb} command{hint}");
        return if hint.is_empty() { Resolution::NoVerb(reason) } else { Resolution::Unknown(reason) };
    }
    match listing(reference, path) {
        Ok(resolution) => resolution,
        Err(reason) if matches!(listing(live, path), Ok(Resolution::Command | Resolution::Group)) && !tables(live, verb).is_empty() => Resolution::Unknown(format!("{reason}, though the live runtime lists it")),
        Err(_) => Resolution::Command,
    }
}

pub(super) fn reference() -> &'static str {
    CLI_REFERENCE
}
