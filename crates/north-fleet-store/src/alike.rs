//! Folds a service's alike messages to one recipient into one follow-up: messages that read the same once
//! the unit, repository or path they name is taken out. The first goes out at once; the rest of its window
//! gather in one group, and the group's one follow-up carries the shared text once, `x N` and every value,
//! replacing the earlier follow-up while the fleet tick has not yet handed it in. A copy re-addressed from
//! an absent lead is grouped under the recipient it reaches, so it joins the same follow-up. On 2026-11-12
//! six lander units each raised one alarm to the coordinator and one copy for the absent lead, twelve
//! messages for one cause.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "alike-groups.json";
pub const WINDOW_MS: u64 = crate::flood::WINDOW_MS;
const HOLE: char = '\u{1}';
const UNIT_SUFFIXES: [&str; 8] = [".service", ".timer", ".socket", ".target", ".scope", ".path", ".mount", ".slice"];

/// A group's follow-up: its text, the group it belongs to, and the spooled follow-up it replaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Folded {
    pub key: String,
    pub text: String,
    pub replaces: Option<String>,
}

/// Whether a message opens a group, so it goes out as it is, or joins an open group's follow-up.
#[derive(Debug, Eq, PartialEq)]
pub enum Join {
    Opens,
    Folds(Folded),
}

#[derive(Default, Deserialize, Serialize)]
struct Group {
    opened_ms: u64,
    template: String,
    rows: Vec<Vec<String>>,
    covered_from: usize,
    follow_up: Option<String>,
}

/// Joins `text`, sent by `sender` to `recipient`, to its group of alike messages; `spooled` says whether a follow-up row is still waiting for the fleet tick.
pub fn join(root: &Path, sender: &str, recipient: &str, text: &str, now_ms: u64, spooled: &dyn Fn(&str) -> bool) -> Join {
    let (template, values) = split(text);
    if values.is_empty() {
        return Join::Opens;
    }
    let mut groups = read(root);
    groups.retain(|_, group| now_ms.saturating_sub(group.opened_ms) < WINDOW_MS);
    let key = format!("{sender}\t{recipient}\t{template}");
    let joined = match groups.get_mut(&key) {
        None => {
            groups.insert(key, Group { opened_ms: now_ms, template, rows: vec![values], covered_from: 0, follow_up: None });
            Join::Opens
        }
        Some(group) => {
            if group.follow_up.as_deref().is_some_and(|id| !spooled(id)) {
                group.covered_from = group.rows.len();
                group.follow_up = None;
            }
            let replaces = group.follow_up.take();
            group.rows.push(values);
            Join::Folds(Folded { key, text: render(&group.template, &group.rows[group.covered_from..]), replaces })
        }
    };
    write(root, &groups);
    joined
}

/// Records that `id` is the spooled follow-up of the group `key`.
pub fn told(root: &Path, key: &str, id: &str) {
    let mut groups = read(root);
    if let Some(group) = groups.get_mut(key) {
        group.follow_up = Some(id.to_owned());
        write(root, &groups);
    }
}

fn render(template: &str, rows: &[Vec<String>]) -> String {
    let mut distinct: Vec<(String, usize)> = Vec::new();
    for row in rows {
        let value = row.join(" ");
        match distinct.iter_mut().find(|(seen, _)| *seen == value) {
            Some((_, times)) => *times += 1,
            None => distinct.push((value, 1)),
        }
    }
    let values: Vec<String> = distinct.iter().map(|(value, times)| if *times > 1 { format!("{value} (x{times})") } else { value.clone() }).collect();
    format!("{} x {}: {}", template.replace(HOLE, "*"), rows.len(), values.join(", "))
}

fn split(text: &str) -> (String, Vec<String>) {
    let (mut template, mut values, mut word) = (String::new(), Vec::new(), String::new());
    for character in text.chars() {
        if character.is_whitespace() {
            word_into(&mut word, &mut template, &mut values);
            template.push(character);
        } else {
            word.push(character);
        }
    }
    word_into(&mut word, &mut template, &mut values);
    (template, values)
}

fn word_into(word: &mut String, template: &mut String, values: &mut Vec<String>) {
    let edge = |character: char| matches!(character, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '\'' | '"' | '`' | '|' | '.');
    let start = word.len() - word.trim_start_matches(edge).len();
    let end = word.trim_end_matches(edge).len();
    if start < end && names(&word[start..end]) {
        template.push_str(&word[..start]);
        template.push(HOLE);
        template.push_str(&word[end..]);
        values.push(word[start..end].to_owned());
    } else {
        template.push_str(word);
    }
    word.clear();
}

fn names(token: &str) -> bool {
    (token.len() > 1 && token.contains('/')) || UNIT_SUFFIXES.iter().any(|suffix| token.ends_with(suffix))
}

fn read(root: &Path) -> BTreeMap<String, Group> {
    fs::read_to_string(root.join(FILE)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn write(root: &Path, groups: &BTreeMap<String, Group>) {
    let path = root.join(FILE);
    let scratch = path.with_extension("json.part");
    if let Ok(json) = serde_json::to_string(groups)
        && fs::write(&scratch, json).is_ok()
    {
        let _ = fs::rename(&scratch, &path);
    }
}
