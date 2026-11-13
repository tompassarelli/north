//! The commit message a worker's launch proposes from its brief, and the launch-time verdict on it: for a brief in the
//! well-specified band whose lane lands through one finish call, the launch runs `north finish --judge-message` in the
//! lane, which scans the message against the fresh deny-list and, only when that scan passes on a public destination,
//! keeps it as the lane's proposal and asks the private-kind judge, so the finish call that leaves out its message
//! finds that verdict cached under the same text (north#1239).
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use north_fleet_store::ssh::quote;

use super::model::{Machine, boxed, item_issues, named_line};
use crate::router::brief::SPEC_EXACT;

pub const CATEGORIES: [&str; 5] = ["mechanical", "docs-policy", "tooling", "bug-known-cause", "feature"];
pub const SPECS: [&str; 1] = [SPEC_EXACT];
pub const SCOPES: [&str; 2] = ["one-file", "module"];
const SUBJECT_CHARS: usize = 72;
const DANGLING: [&str; 14] = ["a", "an", "and", "at", "by", "for", "from", "in", "its", "of", "on", "or", "the", "with"];
const SCAN_WAIT: Duration = Duration::from_secs(20);
const PROPOSED: &str = "proposed";
const REFUSED: &str = "refused";

#[derive(Debug, Eq, PartialEq)]
pub enum Verdict {
    Asked(String),
    Refused,
    Unasked,
}

/// Whether the brief's own `Category:`, `Spec:` and `Scope:` lines put it in the well-specified band.
pub fn in_band(brief: &str) -> bool {
    let declared = |key: &str| named_line(brief, &format!("{key}:")).map(|value| value.trim_matches('`').to_lowercase());
    declared("Category").is_some_and(|category| CATEGORIES.contains(&category.as_str()))
        && declared("Spec").is_some_and(|spec| SPECS.contains(&spec.as_str()))
        && declared("Scope").is_some_and(|scope| SCOPES.contains(&scope.as_str()))
}

/// The message the brief proposes: its own `Commit message:` line as written, else a subject of at most 72 characters
/// from its box in plain words, or its Item's title when it names no box, with the issue number, then the Item's title
/// and its `Item:` trailer.
pub fn message(brief: &str) -> Option<String> {
    if let Some(written) = named_line(brief, "Commit message:").filter(|written| !written.is_empty()) {
        return Some(written.to_owned());
    }
    let issue = item_issues(brief).into_iter().next()?;
    let item = named_line(brief, "Item:").unwrap_or_default();
    let title = item.split_once('(').map(|(_, rest)| rest.rsplit_once(')').map_or(rest, |(title, _)| title).trim()).filter(|title| !title.is_empty());
    let named = boxed(brief).map(|line| plain_box(&line));
    let words = named.as_deref().or(title)?;
    let tail = format!(" ({issue})");
    let subject = format!("{}{tail}", capitalized(&clipped(words, SUBJECT_CHARS - tail.chars().count())));
    let body = title.filter(|_| named.is_some()).map(|title| format!("\n\n{}.", capitalized(title.trim_end_matches('.'))));
    Some(format!("{subject}{}\n\nItem: {issue}", body.unwrap_or_default()))
}

/// What a launch keeps in the lane for its session: the first issue the brief's `Item:` line names, then, when the brief has a `Box:` line and a `Done when:` line with a backticked command, that box and the first such command as `Check:`; None without an item.
pub fn trailers(brief: &str) -> Option<String> {
    let issue = item_issues(brief).into_iter().next()?;
    let mut kept = format!("Item: {issue}");
    let check = named_line(brief, "Done when:").and_then(|line| line.split('`').nth(1)).map(str::trim).filter(|command| !command.is_empty());
    if let (Some(box_text), Some(check)) = (boxed(brief), check) {
        kept.push_str(&format!("\nBox: {box_text}\nCheck: {check}"));
    }
    Some(kept)
}

fn plain_box(line: &str) -> String {
    let quoted = line.split_once('"').and_then(|(_, rest)| rest.split_once('"')).map(|(inside, _)| inside);
    quoted.unwrap_or(line).trim().trim_end_matches('.').to_owned()
}

fn clipped(words: &str, limit: usize) -> String {
    if words.chars().count() <= limit {
        return words.to_owned();
    }
    let mut kept = String::new();
    for word in words.split_whitespace() {
        if kept.chars().count() + word.chars().count() + usize::from(!kept.is_empty()) > limit {
            break;
        }
        if !kept.is_empty() {
            kept.push(' ');
        }
        kept.push_str(word);
    }
    let mut words: Vec<&str> = kept.trim_end_matches([',', ';', ':', '.']).split(' ').collect();
    while words.len() > 1 && words.last().is_some_and(|word| DANGLING.contains(&word.to_lowercase().as_str())) {
        words.pop();
    }
    words.join(" ").trim_end_matches([',', ';', ':', '.']).to_owned()
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| first.to_uppercase().chain(characters).collect())
}

/// Runs `north finish --judge-message` in `cwd`'s lane, here or over ssh on its build host, and returns once its deny-list
/// scan has answered: `Asked` with the message when the scan passed and the judge was asked, which goes on after this
/// returns, `Refused` when the scan refused it, and `Unasked` when nothing was judged.
pub fn judge(message: &str, session: &str, cwd: &Path, north: &Path, machine: Option<&Machine>) -> Verdict {
    match ask("--judge-message", session, message, cwd, north, machine).as_deref() {
        Some(PROPOSED) => Verdict::Asked(message.to_owned()),
        Some(REFUSED) => Verdict::Refused,
        _ => Verdict::Unasked,
    }
}

/// Runs `north finish --brief-trailers` in `cwd`'s lane, which keeps the brief's item, box and Done-when command for `session`, replacing what an earlier launch kept, and clearing it for a brief that names no item, so its finish and hand-in messages take them (north#1367).
pub fn keep_trailers(brief: &str, session: &str, cwd: &Path, north: &Path, machine: Option<&Machine>) {
    ask("--brief-trailers", session, &trailers(brief).unwrap_or_default(), cwd, north, machine);
}

fn ask(flag: &str, session: &str, input: &str, cwd: &Path, north: &Path, machine: Option<&Machine>) -> Option<String> {
    let mut command = match machine {
        None => {
            let mut local = Command::new(north);
            local.args(["finish", flag, session]).current_dir(cwd);
            local
        }
        Some(machine) => {
            let mut ssh = Command::new(super::runtime::binary("ssh"));
            let script = format!("cd {} && {} finish {flag} {}", quote(&cwd.to_string_lossy()), quote(&north.to_string_lossy()), quote(session));
            ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", machine.session_login(), "--", &script]);
            ssh
        }
    };
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let written = child.stdin.take().is_some_and(|mut stdin| stdin.write_all(input.as_bytes()).is_ok());
    let output = child.stdout.take().filter(|_| written)?;
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(output).read_line(&mut line);
        let _ = said.send(line);
        let _ = child.wait();
    });
    heard.recv_timeout(SCAN_WAIT).ok().map(|line| line.trim().to_owned())
}

/// The brief as the worker reads it, with what the launch-time verdict decided about the proposed message.
pub fn told(brief: &str, verdict: &Verdict) -> String {
    match verdict {
        Verdict::Asked(message) => format!("{brief}\n\nProposed commit message, already scanned and its private-kind verdict asked; leave `message` out of the finish call to commit with it exactly:\n{message}\n"),
        Verdict::Refused => format!("{brief}\n\nThe commit message the launch proposed did not pass the deny-list scan; write your own `message`.\n"),
        Verdict::Unasked => brief.to_owned(),
    }
}

#[cfg(test)]
#[path = "proposal_tests.rs"]
mod tests;
