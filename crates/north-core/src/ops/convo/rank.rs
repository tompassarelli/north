use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::hit::Hit;
use super::text::{stem, stems, words};

const STACK: [f64; 3] = [1.0, 0.5, 0.25];
const PHRASE_BOOST: f64 = 3.0;

static CONVO_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:^|[\s;&|(`'"=])convo(?:\s|$)"#).expect("convo call expression"));

fn role_weight(role: &str) -> f64 {
    match role {
        "thinking" | "tool" => 0.8,
        "system" => 0.4,
        _ => 1.0,
    }
}

pub(super) struct Group {
    pub session: String,
    pub best: Hit,
    pub scores: Vec<f64>,
    pub hits: usize,
    pub seen: BTreeSet<String>,
    pub score: f64,
    pub partial: bool,
}

pub(super) fn is_convo_call(hit: &Hit) -> bool {
    hit.role == "tool" && CONVO_CALL.is_match(&hit.text)
}

fn phrase_of(terms: &[String]) -> Option<Regex> {
    if terms.len() < 2 {
        return None;
    }
    let body = terms.iter().map(|term| regex::escape(term)).collect::<Vec<_>>().join(r"\W+");
    Regex::new(&format!(r"(?i)\b{body}\b")).ok()
}

fn weigh(hit: &Hit, phrase: Option<&Regex>) -> f64 {
    let weight = -hit.bm25 * role_weight(&hit.role);
    if phrase.is_some_and(|phrase| phrase.is_match(&hit.text)) { weight * PHRASE_BOOST } else { weight }
}

fn by_weight(left: &Hit, right: &Hit, mine: &HashSet<String>) -> std::cmp::Ordering {
    let key = |hit: &Hit| (mine.contains(&hit.session), -hit.w);
    let (a, b) = (key(left), key(right));
    a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).then(right.id.cmp(&left.id))
}

/// Weighted hits, best first; the caller's own session sorts last.
pub(super) fn rank_messages(hits: &[Hit], mine: &HashSet<String>, keep_calls: bool, terms: &[String]) -> Vec<Hit> {
    let phrase = phrase_of(terms);
    let mut kept: Vec<Hit> = hits
        .iter()
        .filter(|hit| keep_calls || !is_convo_call(hit))
        .map(|hit| Hit { w: weigh(hit, phrase.as_ref()), ..hit.clone() })
        .collect();
    kept.sort_by(|left, right| by_weight(left, right, mine));
    kept
}

/// Groups hits by session: the best hit plus decaying credit for the next two; `coverage` scales by the share of query terms seen.
pub(super) fn rank_sessions(hits: &[Hit], terms: &[String], mine: &HashSet<String>, keep_calls: bool, coverage: bool) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for hit in rank_messages(hits, mine, keep_calls, terms) {
        let at = *index.entry(hit.session.clone()).or_insert_with(|| {
            groups.push(Group { session: hit.session.clone(), best: hit.clone(), scores: Vec::new(), hits: 0, seen: BTreeSet::new(), score: 0.0, partial: false });
            groups.len() - 1
        });
        let group = &mut groups[at];
        group.hits += 1;
        group.scores.push(hit.w);
        if coverage {
            let low = hit.text.to_lowercase();
            group.seen.extend(terms.iter().filter(|term| low.contains(&stem(term))).cloned());
        }
    }
    let keys = stems(terms.iter().map(String::as_str));
    for group in &mut groups {
        group.score = STACK.iter().zip(&group.scores).map(|(weight, score)| weight * score).sum();
        if coverage && !keys.is_empty() {
            let share = stems(group.seen.iter().map(String::as_str)).len() as f64 / keys.len() as f64;
            group.score *= share * share;
        }
    }
    groups.sort_by(|left, right| {
        let key = |group: &Group| (mine.contains(&group.session), -group.score, -group.best.id);
        let (a, b) = (key(left), key(right));
        a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).then(a.2.cmp(&b.2))
    });
    groups
}

fn occurrences(low: &[char], key: &[char]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = 0;
    while !key.is_empty() && at + key.len() <= low.len() {
        if low[at..at + key.len()] == *key {
            found.push(at);
            at += key.len();
        } else {
            at += 1;
        }
    }
    found
}

/// The `width`-character window holding the most distinct query terms.
pub(super) fn snippet(text: &str, terms: &[String], width: usize) -> String {
    let flat = words(text).join(" ");
    let chars: Vec<char> = flat.chars().collect();
    if chars.len() <= width {
        return flat;
    }
    let low: Vec<char> = flat.to_lowercase().chars().collect();
    let keys = stems(terms.iter().map(String::as_str));
    let mut found: Vec<(usize, &String)> = keys.iter().flat_map(|key| occurrences(&low, &key.chars().collect::<Vec<_>>()).into_iter().map(move |at| (at, key))).collect();
    found.sort();
    found.truncate(400);
    let (mut best, mut most) = (0, 0);
    for (index, (start, _)) in found.iter().enumerate() {
        let distinct: HashSet<&String> = found[index..].iter().filter(|(at, _)| *at < start + width * 3 / 4).map(|(_, key)| *key).collect();
        if distinct.len() > most {
            (best, most) = (*start, distinct.len());
        }
    }
    let from = best.saturating_sub(width / 4).min(chars.len() - width);
    let body: String = chars[from..from + width].iter().collect();
    format!("{}{body}{}", if from > 0 { "…" } else { "" }, if from + width < chars.len() { "…" } else { "" })
}

