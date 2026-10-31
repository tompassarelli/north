use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use super::git;

pub enum Predecessor<'a> {
    Issue { owner: Option<&'a str>, name: &'a str, number: &'a str },
    Sha(&'a str),
    Subject(&'a str),
}
pub fn predecessor(follows: &str) -> Predecessor<'_> {
    static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9a-f]{7,40})(?:\s.*)?$").unwrap());
    static ISSUE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:([\w.-]+)/)?([\w.-]+)#(\d+)(?:\s.*)?$").unwrap());
    if let Some(c) = ISSUE.captures(follows) {
        return Predecessor::Issue {
            owner: c.get(1).map(|m| m.as_str()),
            name: c.get(2).unwrap().as_str(),
            number: c.get(3).unwrap().as_str(),
        };
    }
    if let Some(c) = SHA.captures(follows) {
        return Predecessor::Sha(c.get(1).unwrap().as_str());
    }
    Predecessor::Subject(follows)
}
pub fn subject_on_main(repo: &Path, subject: &str) -> Result<bool, String> {
    let output = git::run(repo, &["log", "-n2000", "--format=%B%x00", "refs/remotes/origin/main"]).map_err(|error| error.to_string())?;
    let log = String::from_utf8_lossy(&output.stdout);
    Ok(log
        .split('\0')
        .any(|m| m.trim().split('\n').next() == Some(subject)))
}
/// The `Follows:` line of a queued box's brief, as `threads queue follow` reads it.
pub fn follows_line(brief: &str) -> Option<String> {
    static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\W*Follows:[ \t]*`?(.+?)`?[ \t]*$").unwrap());
    LINE.captures(brief).map(|found| found[1].trim().to_owned())
}

pub const ACCEPTED: &str = "an issue (`repo#N` or `owner/repo#N`), a commit SHA of 7 to 40 hex digits, or the subject of a commit on the repo's origin/main, each optionally followed by a note";

/// Whether a Follows line names something a release can wait on: an issue, a SHA, or the subject
/// of a commit already on `repo`'s origin/main.
pub fn vet(follows: &str, repo: Option<&Path>) -> Result<(), String> {
    let refused = || format!("Follows: {follows} names no issue, SHA or landed subject; use {ACCEPTED}");
    match predecessor(follows) {
        Predecessor::Subject(subject) => repo.and_then(|repo| subject_on_main(repo, subject).ok()).filter(|found| *found).map(|_| ()).ok_or_else(refused),
        _ => Ok(()),
    }
}
