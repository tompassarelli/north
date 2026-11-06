use std::sync::OnceLock;

use regex::Regex;

use crate::boxes::Item;

#[derive(Debug, Eq, PartialEq)]
pub enum Wait {
    Issue(String),
    Box(String),
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Waits {
    pub on: Vec<Wait>,
    pub unparsed: Vec<String>,
}

const CLAUSE_LIMIT: usize = 90;

fn trigger() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)\b(waits?\s+(?:on|for)|waiting\s+(?:on|for)|blocked\s+by|depends\s+on|follows|after|needs)\s+").unwrap_or_else(|error| panic!("{error}")))
}

fn issue_reference() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?:\b([A-Za-z][A-Za-z0-9_-]*))?#(\d+)\b").unwrap_or_else(|error| panic!("{error}")))
}

fn position_reference() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)\bbox(?:es)?\s+(\d+(?:(?:\s*,\s*(?:and\s+)?|\s+and\s+)\d+)*)").unwrap_or_else(|error| panic!("{error}")))
}

fn label_reference() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\b(?:[A-Z]{1,3}\d+[a-z]?|\d{1,2}-\d{1,3}|(?:Prune|Split|Box|Step|Phase|Part)\s+\d+[a-z]?)\b").unwrap_or_else(|error| panic!("{error}")))
}

fn box_word() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)\bbox(?:es)?\b").unwrap_or_else(|error| panic!("{error}")))
}

fn clause(after: &str) -> &str {
    let mut end = after.len().min(CLAUSE_LIMIT);
    while !after.is_char_boundary(end) {
        end -= 1;
    }
    let window = &after[..end];
    let stop = window.char_indices().find(|&(at, character)| match character {
        ';' | '(' | ')' | '\n' => true,
        '.' => window[at + 1..].chars().next().is_none_or(char::is_whitespace),
        _ => false,
    });
    &window[..stop.map_or(window.len(), |(at, _)| at)]
}

fn bounded(clause: &str, start: usize, end: usize) -> bool {
    let before = clause[..start].chars().next_back();
    let after = clause[end..].chars().next();
    !before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '/') && !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '%')
}

fn leading_label(text: &str) -> &str {
    let text = text.trim_start_matches(|c: char| c == '*' || c == '`' || c == '(' || c.is_whitespace());
    let end = text.find(|c: char| !(c.is_alphanumeric() || c == '-' || c == ' ')).unwrap_or(text.len());
    text[..end].trim_end()
}

fn carries_label(text: &str, label: &str) -> bool {
    let head = leading_label(text);
    head == label || head.strip_prefix(label).is_some_and(|rest| rest.starts_with(' '))
}

struct Reading<'a> {
    item: &'a Item,
    issue: &'a str,
    siblings: &'a [Item],
    seen: usize,
    waits: Vec<Wait>,
    missing: Vec<String>,
}

impl<'a> Reading<'a> {
    fn push(&mut self, wait: Wait) {
        self.seen += 1;
        if !self.waits.contains(&wait) {
            self.waits.push(wait);
        }
    }

    fn sibling(&mut self, other: Option<&Item>, name: &str) {
        match other {
            Some(other) if other.head != self.item.head => self.push(Wait::Box(other.id(self.issue))),
            Some(_) => self.seen += 1,
            None => self.missing.push(name.to_owned()),
        }
    }

    fn read(&mut self, phrase: &str) {
        let repo = self.issue.split('#').next().unwrap_or_default();
        for reference in issue_reference().captures_iter(phrase) {
            let target = format!("{}#{}", reference.get(1).map_or(repo, |name| name.as_str()), &reference[2]);
            if target == self.issue {
                self.seen += 1;
            } else {
                self.push(Wait::Issue(target));
            }
        }
        let siblings = self.siblings;
        for reference in position_reference().captures_iter(phrase) {
            for number in reference[1].split(|c: char| !c.is_ascii_digit()).filter(|part| !part.is_empty()) {
                let index = number.parse::<usize>().ok().and_then(|position| position.checked_sub(1));
                self.sibling(index.and_then(|index| siblings.get(index)), &format!("box {number}"));
            }
        }
        for label in label_reference().find_iter(phrase).filter(|label| bounded(phrase, label.start(), label.end())) {
            self.sibling(siblings.iter().find(|other| carries_label(&other.text, label.as_str())), label.as_str());
        }
    }
}

pub fn of(item: &Item, issue: &str, siblings: &[Item]) -> Waits {
    let mut waits = Waits::default();
    for found in trigger().captures_iter(&item.text) {
        let Some(whole) = found.get(0) else { continue };
        let phrase = clause(&item.text[whole.end()..]);
        let mut reading = Reading { item, issue, siblings, seen: 0, waits: Vec::new(), missing: Vec::new() };
        reading.read(phrase);
        let referenced = reading.seen > 0 || !reading.missing.is_empty();
        let unreadable = !reading.missing.is_empty() || (reading.seen == 0 && box_word().is_match(phrase));
        for wait in reading.waits {
            if !waits.on.contains(&wait) {
                waits.on.push(wait);
            }
        }
        let needs_a_reference = found[1].eq_ignore_ascii_case("needs") && !referenced;
        if unreadable && !needs_a_reference {
            waits.unparsed.push(format!("{} {}", &found[1], phrase.trim_end()));
        }
    }
    waits
}

pub fn follows(body: &str, repo: &str) -> (Vec<String>, Vec<String>) {
    let mut named = Vec::new();
    let mut unparsed = Vec::new();
    for line in body.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '>', ' ']);
        let Some((key, value)) = line.split_once(':') else { continue };
        if !key.trim().trim_matches('*').eq_ignore_ascii_case("Follows") {
            continue;
        }
        let before = named.len();
        for reference in issue_reference().captures_iter(value) {
            named.push(format!("{}#{}", reference.get(1).map_or(repo, |name| name.as_str()), &reference[2]));
        }
        if named.len() == before {
            unparsed.push(format!("Follows:{value}"));
        }
    }
    (named, unparsed)
}
