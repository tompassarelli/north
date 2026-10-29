use crate::platform::sqlite::{self, Access};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

const TITLE_MIN_CHARS: usize = 20;
const TITLE_MIN_WORDS: usize = 3;
const ITEM_TITLES: &str = "SELECT title FROM items";

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").unwrap());

pub(super) fn quotes_native_title(text: &str, databases: &[PathBuf]) -> bool {
    let haystack = normalized(text);
    native_titles(databases)
        .iter()
        .any(|title| haystack.contains(&format!(" {title} ")))
}

fn normalized(text: &str) -> String {
    let lowered = text.to_lowercase();
    let words: Vec<&str> = WORD.find_iter(&lowered).map(|m| m.as_str()).collect();
    format!(" {} ", words.join(" "))
}

pub(super) fn databases() -> Vec<PathBuf> {
    let state = PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".local/state/threads");
    let mut paths: Vec<PathBuf> = std::env::var_os("THREADS_DB")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .into_iter()
        .chain([state.join("threads.db"), state.join("snapshot.db")])
        .filter(|path| path.exists())
        .collect();
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

fn native_titles(databases: &[PathBuf]) -> Vec<String> {
    let mut titles = Vec::new();
    for path in databases {
        let Ok(rows) = sqlite::rows(path, Access::ReadOnly, Duration::ZERO, ITEM_TITLES) else {
            continue;
        };
        titles.extend(
            rows.iter()
                .filter_map(|row| row.as_object())
                .flat_map(|columns| columns.values())
                .filter_map(|value| value.as_str())
                .map(|title| normalized(title).trim().to_owned())
                .filter(|title| {
                    title.chars().count() >= TITLE_MIN_CHARS
                        && title.split(' ').count() >= TITLE_MIN_WORDS
                }),
        );
    }
    titles
}
