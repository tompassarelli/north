//! The words a brief's facet line names, and which of them are local needs that keep a box off
//! code-only targets. A chrome-profile is the laptop's sign-ins; native-client and gpu are native work.
use super::field;

pub(super) const LOCAL_NEEDS: [&str; 12] = [
    "warcraft", "wc3", "client", "native", "sign-ins", "private", "local-state", "tui", "fleet", "live", "desktop", "laptop",
];

const SYNONYMS: [(&str, &str); 3] = [("chrome-profile", "sign-ins"), ("native-client", "native"), ("gpu", "native")];

pub fn words(text: &str, key: &str) -> Vec<String> {
    field(text, key)
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',' || c == '|' || c == '/')
        .filter(|s| !s.is_empty())
        .map(|word| SYNONYMS.iter().find(|(name, _)| *name == word).map_or(word, |(_, need)| need).to_owned())
        .collect()
}
pub(super) fn is_local_need(word: &str) -> bool {
    LOCAL_NEEDS.contains(&word)
}
