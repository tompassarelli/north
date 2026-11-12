use std::collections::{BTreeMap, BTreeSet};

use crate::proc::{git, run};
use crate::task::{Task, Tree, commit_time};

const STOPWORDS: &[&str] = &[
    "the", "that", "this", "with", "from", "then", "than", "into", "stay", "stays", "should", "instead", "keep", "each", "when", "what", "make", "only", "first", "other", "same", "their", "there", "they", "them", "have", "been",
    "does", "also", "over", "under", "before", "after", "rest", "order", "just", "like", "more", "most", "some", "such", "which", "while", "without",
];
pub const GREP_WORDS: usize = 24;

fn words(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let (mut found, mut seen, mut at) = (Vec::new(), BTreeSet::new(), 0);
    while at < bytes.len() {
        if !bytes[at].is_ascii_lowercase() {
            at += 1;
            continue;
        }
        let end = (at..bytes.len()).find(|&i| !(bytes[i].is_ascii_lowercase() || bytes[i].is_ascii_digit() || bytes[i] == b'_')).unwrap_or(bytes.len());
        let word = &lower[at..end];
        if word.len() >= 4 && !STOPWORDS.contains(&word) && seen.insert(word.to_owned()) {
            found.push(word.to_owned());
        }
        at = end;
    }
    found.truncate(GREP_WORDS);
    found
}

pub fn grep_rank(task: &Task) -> Result<Vec<String>, String> {
    let prefix = format!("{}:", task.base);
    let mut hits: BTreeMap<String, (u32, u64)> = BTreeMap::new();
    for word in words(&task.text) {
        let counted = git(&task.repo, &["grep", "-c", "-i", "-F", "-e", &word, &task.base, "--", "."], None).map(|ran| ran.text()).unwrap_or_default();
        for line in counted.lines() {
            let Some((path, count)) = line.strip_prefix(&prefix).and_then(|rest| rest.rsplit_once(':')) else { continue };
            let entry = hits.entry(path.to_owned()).or_default();
            entry.0 += 1;
            entry.1 += count.parse::<u64>().unwrap_or(0);
        }
    }
    let mut ranked: Vec<_> = hits.into_iter().collect();
    ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(b.1.1.cmp(&a.1.1)).then(a.0.cmp(&b.0)));
    Ok(ranked.into_iter().map(|(path, _)| path).collect())
}

pub fn graph_output(task: &Task) -> Result<String, String> {
    let when = commit_time(&task.repo, &task.base)?;
    let ran = run("north", &["graph", "ground", "query", "--query", &task.text], &[("NORTH_GROUNDING_AS_OF", &when)], &[], None);
    Ok(ran.text())
}

pub fn graph_rank(output: &str, tree: &Tree) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ranked = Vec::new();
    for token in output.split(|c: char| !(c.is_ascii_alphanumeric() || "_./:-".contains(c))) {
        let bare = token.rsplit_once(':').map_or(token, |(_, rest)| rest).trim_matches(|c| c == '.' || c == '-');
        if tree.has(bare) && seen.insert(bare.to_owned()) {
            ranked.push(bare.to_owned());
        }
    }
    ranked
}
