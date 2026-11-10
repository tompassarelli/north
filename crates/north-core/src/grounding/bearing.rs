use super::*;
use compose::{HOOK_FLOOR, category, code, retrieve_db, shown};
use std::path::Path;
use store::{Hit, Index};

const NAMED: usize = 5;
const SUBJECT_TERMS: usize = 2;
const ASKING: &str = r"(?i)\b(?:recommend\w*|fix(?:es|ed)?)\b";
const EDIT_WORDS: [&str; 64] = [
    "add", "adds", "adding", "added", "remove", "removes", "removing", "removed", "delete", "deleting", "deleted", "rename",
    "renames", "renaming", "renamed", "change", "changes", "changing", "changed", "update", "updates", "updating", "updated", "edit",
    "editing", "edited", "move", "moving", "moved", "line", "lines", "blank", "end", "start", "top", "bottom",
    "heading", "headings", "title", "titles", "readme", "section", "sections", "paragraph", "text", "word", "words", "wording",
    "sentence", "file", "files", "doc", "docs", "comment", "comments", "note", "notes", "typo", "format", "formatting",
    "page", "list", "entry", "entries",
];

pub fn records(paragraph: &str, now: f64) -> Vec<String> {
    records_at(paragraph, &north_graph::default_path(), now)
}

pub fn records_at(paragraph: &str, index: &Path, now: f64) -> Vec<String> {
    let Ok(mut db) = Index::open_at(index) else { return Vec::new() };
    records_in(paragraph, &mut db, now)
}

pub fn records_in(paragraph: &str, db: &mut Index, now: f64) -> Vec<String> {
    let subject = re(ASKING).replace_all(paragraph, " ");
    let Ok((hits, _)) = retrieve_db(&subject, db, now, 80, 0.0, "") else { return Vec::new() };
    let ledger = distill::ledger_path();
    let mut named: Vec<String> = Vec::new();
    for hit in hits.iter().filter(|hit| bears(hit)) {
        let place = match category(hit).as_str() {
            "Decisions" => match cite(&hit.reference, &ledger) {
                Some(place) => place,
                None => continue,
            },
            _ => shown(&hit.reference),
        };
        if !named.contains(&place) {
            named.push(place);
        }
        if named.len() == NAMED {
            break;
        }
    }
    named
}

fn cite(reference: &str, ledger: &Path) -> Option<String> {
    let id = reference.strip_prefix("ledger:")?;
    let row = distill::show_at(id, ledger).ok().flatten()?;
    Some(format!("{} — {}", shown(reference), row.summary()))
}

fn bears(hit: &Hit) -> bool {
    hit.cover >= HOOK_FLOOR && subject_terms(hit) >= SUBJECT_TERMS && matches!(category(hit).as_str(), "Decisions" | "Policy") && !code(hit)
}

fn subject_terms(hit: &Hit) -> usize {
    hit.matched.iter().filter(|term| !EDIT_WORDS.contains(&term.as_str())).count()
}

#[cfg(test)]
#[path = "bearing_tests.rs"]
mod tests;
