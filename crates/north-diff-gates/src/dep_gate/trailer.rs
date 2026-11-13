use super::{Urgent, matches};
use regex::Regex;
use std::collections::BTreeSet;

pub fn trailer_lines(messages: &[String]) -> Vec<&str> {
    let paragraphs = Regex::new(r"\n\s*\n").unwrap();
    let separators = Regex::new(r"\r\n|[\n\r\x0b\x0c\x1c-\x1e\u{85}\u{2028}\u{2029}]").unwrap();
    messages
        .iter()
        .flat_map(|message| {
            let parts: Vec<_> = paragraphs.split(message.trim()).collect();
            if parts.len() < 2 {
                return Vec::new();
            }
            let mut lines: Vec<_> = separators.split(parts.last().unwrap()).collect();
            if lines.last() == Some(&"") {
                lines.pop();
            }
            if lines
                .iter()
                .any(|line| !matches(r"\A[A-Za-z][A-Za-z0-9-]*:\s+.+\z", line))
            {
                Vec::new()
            } else {
                lines
            }
        })
        .collect()
}

pub(super) fn dependency_reasons(messages: &[String]) -> BTreeSet<String> {
    let re = Regex::new(r"\ADependency:\s+(\S+)\s+(?:—|-)\s+(.+)\z").unwrap();
    trailer_lines(messages)
        .into_iter()
        .filter_map(|line| {
            re.captures(line)
                .and_then(|c| (!c[2].trim().is_empty()).then(|| c[1].to_owned()))
        })
        .collect()
}

pub(super) fn urgent_dependencies(messages: &[String]) -> Urgent {
    let re = Regex::new(r"\ADependency-Urgent:\s+(\S+)@(\S+)\s+(\S+)\z").unwrap();
    trailer_lines(messages)
        .into_iter()
        .filter_map(|line| {
            re.captures(line)
                .map(|c| (c[1].to_owned(), c[2].to_owned()))
        })
        .collect()
}
