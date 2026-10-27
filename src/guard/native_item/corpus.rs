use crate::platform::sqlite::{self, Access};
use regex::Regex;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

const SHINGLE: usize = 6;
const SENSITIVE_MIN_CHARS: usize = 6;
const ITEM_TEXT: &str = "SELECT title, body FROM items";
const NOTE_TEXT: &str = "SELECT json_extract(data, '$.text') AS text FROM events WHERE kind = 'note' AND json_extract(data, '$.text') IS NOT NULL";

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").unwrap());
static SENSITIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\w@]+(?:[.+/-][\w@]+)*").unwrap());
static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d\d-\d\d$").unwrap());

pub(super) fn quotes_native_text(text: &str, databases: &[PathBuf]) -> bool {
    let shingles = word_shingles(text);
    let tokens = sensitive_tokens(text);
    if shingles.is_empty() && tokens.is_empty() {
        return false;
    }
    native_texts(databases).iter().any(|native| {
        word_shingles(native).iter().any(|s| shingles.contains(s))
            || sensitive_tokens(native).iter().any(|t| tokens.contains(t))
    })
}

fn word_shingles(text: &str) -> HashSet<String> {
    let lowered = text.to_lowercase();
    let words: Vec<&str> = WORD.find_iter(&lowered).map(|m| m.as_str()).collect();
    words.windows(SHINGLE).map(|window| window.join(" ")).collect()
}

fn sensitive_tokens(text: &str) -> HashSet<String> {
    SENSITIVE
        .find_iter(&text.to_lowercase())
        .map(|m| m.as_str().to_owned())
        .filter(|token| {
            token.chars().count() >= SENSITIVE_MIN_CHARS
                && (token.contains('@') || token.chars().any(|c| c.is_ascii_digit()))
                && !DATE.is_match(token)
        })
        .collect()
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
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

fn native_texts(databases: &[PathBuf]) -> Vec<String> {
    let mut texts = Vec::new();
    for path in databases {
        for query in [ITEM_TEXT, NOTE_TEXT] {
            let Ok(rows) = sqlite::rows(path, Access::ReadOnly, Duration::ZERO, query) else {
                continue;
            };
            texts.extend(
                rows.iter()
                    .filter_map(|row| row.as_object())
                    .flat_map(|columns| columns.values())
                    .filter_map(|value| value.as_str())
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
            );
        }
    }
    texts
}
