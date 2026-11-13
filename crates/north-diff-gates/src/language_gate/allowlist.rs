use super::{ALLOWLIST, Result};
use serde::Deserialize;
use std::collections::{BTreeSet, VecDeque};
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum Class {
    Python,
    Script,
    Shell,
}

pub(super) fn classify(path: &Path, content: &[u8]) -> Option<Class> {
    match path.extension().and_then(|value| value.to_str()) {
        Some("py") => Some(Class::Python),
        Some("ts" | "tsx" | "mts" | "cts" | "js" | "mjs" | "cjs") => Some(Class::Script),
        Some("sh" | "bash") => Some(Class::Shell),
        None => {
            let line = content
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or_default();
            let line = std::str::from_utf8(line).ok()?.strip_prefix("#!")?;
            let mut words = line.split_whitespace();
            let executable = Path::new(words.next()?).file_name()?.to_str()?;
            let executable = if executable == "env" {
                words.find(|word| !word.starts_with('-') && !word.contains('='))?
            } else {
                executable
            };
            match executable {
                "python" | "python3" => Some(Class::Python),
                "sh" | "bash" | "zsh" | "dash" => Some(Class::Shell),
                _ => None,
            }
        }
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct Entry {
    pub(super) glob: String,
    pub(super) class: Class,
    pub(super) gap: String,
    pub(super) provenance: String,
    pub(super) port_owner: Option<String>,
    pub(super) port_target: Option<String>,
    pub(super) commitment: Option<String>,
    pub(super) created: Option<String>,
}

#[derive(Default, Deserialize)]
pub(super) struct Allowlist {
    #[serde(default)]
    pub(super) ratchet: Vec<Class>,
    pub(super) entries: Vec<Entry>,
}

impl Allowlist {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        let list: Self =
            serde_json::from_slice(bytes).map_err(|error| format!("{ALLOWLIST}: {error}"))?;
        for entry in &list.entries {
            if entry.gap.trim().is_empty()
                || entry.provenance.trim().is_empty()
                || entry.glob.is_empty()
            {
                return Err(format!(
                    "{ALLOWLIST}: invalid entry: glob, gap and provenance must be nonempty"
                ));
            }
        }
        Ok(list)
    }

    pub(super) fn allows(&self, path: &str, class: Class) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.class == class && glob_matches(&entry.glob, path))
    }
}

#[derive(Clone, Copy)]
pub(super) enum Token {
    Literal(char),
    Any,
    Star,
    All,
    Directory,
    DirectoryBody,
}

pub(super) fn tokens(glob: &str) -> Vec<Token> {
    let mut chars = glob.chars().peekable();
    let mut result = Vec::new();
    while let Some(ch) = chars.next() {
        let token = match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    result.push(Token::Directory);
                    Token::DirectoryBody
                } else {
                    Token::All
                }
            }
            '*' => Token::Star,
            '?' => Token::Any,
            _ => Token::Literal(ch),
        };
        result.push(token);
    }
    result
}

pub(super) fn closure(pattern: &[Token], mut states: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut pending: Vec<_> = states.iter().copied().collect();
    while let Some(index) = pending.pop() {
        let next = match pattern.get(index) {
            Some(Token::Star | Token::All) => Some(index + 1),
            Some(Token::Directory) => Some(index + 2),
            _ => None,
        };
        if let Some(next) = next
            && states.insert(next)
        {
            pending.push(next);
        }
    }
    states
}

pub(super) fn advance(pattern: &[Token], states: &BTreeSet<usize>, ch: char) -> BTreeSet<usize> {
    let mut next = BTreeSet::new();
    for &index in states {
        match pattern.get(index) {
            Some(Token::Literal(expected)) if ch == *expected => {
                next.insert(index + 1);
            }
            Some(Token::Any) if ch != '/' => {
                next.insert(index + 1);
            }
            Some(Token::Star) if ch != '/' => {
                next.insert(index);
            }
            Some(Token::All) => {
                next.insert(index);
            }
            Some(Token::Directory) => {
                next.insert(index + 1);
                if ch == '/' {
                    next.insert(index);
                }
            }
            Some(Token::DirectoryBody) => {
                next.insert(index);
                if ch == '/' {
                    next.insert(index - 1);
                }
            }
            _ => {}
        }
    }
    closure(pattern, next)
}

pub(super) fn glob_matches(glob: &str, path: &str) -> bool {
    let pattern = tokens(glob);
    let states = path
        .chars()
        .fold(closure(&pattern, BTreeSet::from([0])), |states, ch| {
            advance(&pattern, &states, ch)
        });
    states.contains(&pattern.len())
}

pub(super) fn covered_by(entry: &Entry, old: &Allowlist) -> bool {
    let new = tokens(&entry.glob);
    let old: Vec<_> = old
        .entries
        .iter()
        .filter(|other| other.class == entry.class)
        .map(|other| tokens(&other.glob))
        .collect();
    let mut alphabet = BTreeSet::from(['/']);
    for token in new.iter().chain(old.iter().flatten()) {
        if let Token::Literal(ch) = token {
            alphabet.insert(*ch);
        }
    }
    if let Some(other) = (1..=0x10ffff)
        .filter_map(char::from_u32)
        .find(|ch| !alphabet.contains(ch))
    {
        alphabet.insert(other);
    }
    let start = (
        closure(&new, BTreeSet::from([0])),
        old.iter()
            .map(|pattern| closure(pattern, BTreeSet::from([0])))
            .collect::<Vec<_>>(),
    );
    let mut seen = BTreeSet::from([start.clone()]);
    let mut queue = VecDeque::from([start]);
    while let Some((new_states, old_states)) = queue.pop_front() {
        if new_states.contains(&new.len())
            && !old
                .iter()
                .zip(&old_states)
                .any(|(pattern, states)| states.contains(&pattern.len()))
        {
            return false;
        }
        for &ch in &alphabet {
            let next = (
                advance(&new, &new_states, ch),
                old.iter()
                    .zip(&old_states)
                    .map(|(pattern, states)| advance(pattern, states, ch))
                    .collect::<Vec<_>>(),
            );
            if !next.0.is_empty() && seen.insert(next.clone()) {
                queue.push_back(next);
            }
        }
    }
    true
}
