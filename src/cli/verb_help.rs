//! The dispatcher answers `--help` for every verb before any verb runs, so a probe such as firn's
//! unit-flags check never starts a sampler, a post or a launch: from the contract a verb publishes
//! when it has one, else from its help.txt lines.
use super::CLI_HELP;
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
    (&["lane-gc"], GC_USAGE),
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
    let commands = CLI_HELP.split_once("Commands:\n").map_or("", |(_, rest)| rest);
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
    if let Some((_, contract)) = CONTRACTS.iter().filter(|(prefix, _)| starts_with(path, prefix)).max_by_key(|(prefix, _)| prefix.len()) {
        return Some(format!("{}\n", contract.trim_end()));
    }
    let entries = entries();
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
