use super::scan::{Class, classify};
use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

pub fn blank(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' { '\n' } else { ' ' })
        .collect()
}

pub(super) fn heredoc(text: &str, start: usize) -> Option<(usize, String, bool)> {
    let re = Regex::new(r#"<<-?[\s\x1c-\x1f]*(['"]?)([\p{L}\p{N}_]+)"#).unwrap();
    let rest = &text[start..];
    let classes = classify(rest);
    for m in re.captures_iter(rest) {
        let whole = m.get(0).unwrap();
        let at = rest[..whole.start()].chars().count();
        if text[..start + whole.start()].ends_with('<')
            || classes[at..at + 2] != [Class::Code, Class::Code]
        {
            continue;
        }
        let end = start + whole.end();
        let quote = &m[1];
        if quote.is_empty() || text[end..].starts_with(quote) {
            return Some((end + quote.len(), m[2].to_owned(), !quote.is_empty()));
        }
    }
    None
}

pub(super) fn terminator(text: &str, start: usize, delimiter: &str) -> Option<(usize, usize)> {
    let re = Regex::new(&format!(r"(?m)^[ \t]*{}[ \t]*$", regex::escape(delimiter))).unwrap();
    re.find_at(text, start).map(|m| (m.start(), m.end()))
}

pub fn heredoc_bodies(text: &str) -> Vec<Range<usize>> {
    let mut bodies = Vec::new();
    let mut i = 0;
    while let Some((end, delimiter, _)) = heredoc(text, i) {
        let Some(line_end) = text[end..].find('\n').map(|n| end + n + 1) else {
            break;
        };
        match terminator(text, line_end, &delimiter) {
            Some((start, _)) => {
                bodies.push(line_end..start);
                i = start;
            }
            None => {
                bodies.push(line_end..text.len());
                break;
            }
        }
    }
    bodies
}

pub fn strip_heredocs(text: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    for body in heredoc_bodies(text) {
        out.push_str(&text[i..body.start]);
        out.push_str(&blank(&text[body.clone()]));
        i = body.end;
    }
    out.push_str(&text[i..]);
    out
}
pub fn strip_heredoc_bodies(command: &str) -> String {
    static HEREDOC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"<<-?[\s\x1c-\x1f]*['"]?([\p{L}\p{N}_]+)['"]?"#).unwrap());
    let mut out = String::new();
    let mut index = 0;
    for m in HEREDOC.captures_iter(command) {
        let end = m.get(0).unwrap().end();
        if index <= end {
            out.push_str(&command[index..end]);
        }
        let closing = Regex::new(&format!(
            r"(?m)^[\s\x1c-\x1f]*{}[\s\x1c-\x1f]*$",
            regex::escape(&m[1])
        ))
        .unwrap();
        index = end
            + closing
                .find(&command[end..])
                .map_or(command.len() - end, |found| found.end());
    }
    out.push_str(&command[index..]);
    out
}
