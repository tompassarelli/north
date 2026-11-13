use std::collections::BTreeSet;

pub(super) struct LaneCommit {
    pub(super) text: String,
    pub(super) trailers: Vec<String>,
    pub(super) merge: bool,
}

fn is_trailer(line: &str) -> bool {
    let Some((key, value)) = line.split_once(": ") else { return false };
    !value.trim().is_empty()
        && key.chars().next().is_some_and(|first| first.is_ascii_alphanumeric())
        && key.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
}

pub(super) fn split_trailers(message: &str) -> (String, Vec<String>) {
    let message = message.trim_end();
    let Some((head, last)) = message.rsplit_once("\n\n") else { return (message.to_owned(), Vec::new()) };
    let lines: Vec<&str> = last.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.first().is_none_or(|line| !is_trailer(line))
        || !lines.iter().all(|line| is_trailer(line) || line.starts_with([' ', '\t']))
    {
        return (message.to_owned(), Vec::new());
    }
    let mut trailers: Vec<String> = Vec::new();
    for line in lines {
        match trailers.last_mut() {
            Some(previous) if line.starts_with([' ', '\t']) => {
                previous.push(' ');
                previous.push_str(line.trim());
            }
            _ => trailers.push(line.trim_end().to_owned()),
        }
    }
    (head.trim_end().to_owned(), trailers)
}

pub(super) fn message(commits: &[&LaneCommit], supplied: Option<&str>) -> String {
    let mut text = match supplied {
        Some(supplied) => split_trailers(supplied).0,
        None => {
            let finished: Vec<&str> = commits.iter().filter(|commit| !unfinished_subject(commit.text.lines().next().unwrap_or_default())).map(|commit| commit.text.as_str()).collect();
            if finished.is_empty() { commits[0].text.clone() } else { finished.join("\n\n") }
        }
    };
    let mut seen = BTreeSet::new();
    let supplied_trailers = supplied.map(|supplied| split_trailers(supplied).1).unwrap_or_default();
    let trailers: Vec<&String> = supplied_trailers
        .iter()
        .chain(commits.iter().flat_map(|commit| commit.trailers.iter()))
        .filter(|trailer| seen.insert(trailer.as_str()))
        .collect();
    if !trailers.is_empty() {
        text.push_str("\n\n");
        text.push_str(&trailers.iter().map(|trailer| trailer.as_str()).collect::<Vec<_>>().join("\n"));
    }
    text.push('\n');
    text
}

/// A landing subject that calls the work unfinished: it starts with the word WIP or says the work is not written.
pub fn unfinished_subject(subject: &str) -> bool {
    let lower = subject.trim().to_lowercase();
    let wip = lower.strip_prefix("wip").is_some_and(|rest| rest.chars().next().is_none_or(|next| !next.is_alphanumeric()));
    wip || lower.contains("not written") || lower.contains("not yet written")
}
