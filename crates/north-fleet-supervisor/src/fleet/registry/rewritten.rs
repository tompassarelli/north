//! The issues a closed issue was rewritten as: the 24-hour rule closes an issue a lead holds and its successors are named in a `rewritten as REPO#N` comment on it, so a lead's launch on a successor finds it in the lead's record (north#1495).

/// Reads the comments of an issue GitHub has closed.
pub trait ClosedIssues: Send + Sync {
    /// The comments on `task`, `[OWNER/]REPO#N`, when it is closed; none while it is open or cannot be read.
    fn closed_comments(&self, task: &str) -> Vec<String>;
}

pub struct Unread;

impl ClosedIssues for Unread {
    fn closed_comments(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

/// The `REPO#N` issues each `rewritten as` line of `comments` names, owner prefix dropped.
pub(super) fn rewritten_as(comments: &[String]) -> Vec<String> {
    comments
        .iter()
        .flat_map(|comment| comment.lines())
        .filter_map(|line| line.to_ascii_lowercase().find("rewritten as").map(|at| line[at + "rewritten as".len()..].to_owned()))
        .flat_map(|tail| tail.split(|c: char| c.is_whitespace() || c == ',' || c == ';').map(issue).collect::<Vec<_>>())
        .flatten()
        .collect()
}

/// `word` as `REPO#N` when it names an issue, trailing punctuation and owner prefix dropped.
pub(super) fn issue(word: &str) -> Option<String> {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let word = word.rsplit('/').next().unwrap_or(word);
    let (repository, number) = word.split_once('#')?;
    (!repository.is_empty() && !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then(|| word.to_lowercase())
}
