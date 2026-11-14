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

fn trailer_block(paragraph: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = paragraph.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.first().is_none_or(|line| !is_trailer(line))
        || !lines.iter().all(|line| is_trailer(line) || line.starts_with([' ', '\t']))
    {
        return None;
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
    Some(trailers)
}

pub(super) fn split_trailers(message: &str) -> (String, Vec<String>) {
    let mut head = message.trim_end();
    let mut blocks = Vec::new();
    while let Some((rest, last)) = head.rsplit_once("\n\n")
        && let Some(block) = trailer_block(last)
    {
        blocks.push(block);
        head = rest.trim_end();
    }
    (head.to_owned(), blocks.into_iter().rev().flatten().collect())
}

fn key(trailer: &str) -> String {
    trailer.split_once(": ").map_or(trailer, |(key, _)| key).to_ascii_lowercase()
}

pub(super) fn message(commits: &[&LaneCommit], supplied: Option<&str>) -> String {
    let (supplied_text, supplied_trailers) = supplied.map(split_trailers).unzip();
    let supplied_trailers = supplied_trailers.unwrap_or_default();
    let mut text = match supplied_text {
        Some(supplied_text) => supplied_text,
        None => {
            let finished: Vec<&str> = commits.iter().filter(|commit| !unfinished_subject(commit.text.lines().next().unwrap_or_default())).map(|commit| commit.text.as_str()).collect();
            if finished.is_empty() { commits[0].text.clone() } else { finished.join("\n\n") }
        }
    };
    let named: BTreeSet<String> = supplied_trailers.iter().map(|trailer| key(trailer)).collect();
    let mut seen = BTreeSet::new();
    let trailers: Vec<&String> = supplied_trailers
        .iter()
        .chain(commits.iter().flat_map(|commit| commit.trailers.iter()).filter(|trailer| !named.contains(&key(trailer))))
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
