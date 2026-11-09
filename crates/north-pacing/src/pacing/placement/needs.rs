//! The words a brief's facet line names, and which of them are local needs that keep a box off
//! code-only targets. A chrome-profile is the laptop's sign-ins; native-client and gpu are native work.
//! A box's own `Needs:` line declares its needs (`declared`); its prose is never read for them.
use super::field;
use regex::Regex;
use std::sync::LazyLock;

const LOCAL_NEEDS: [&str; 13] = [
    "warcraft", "wc3", "client", "native", "sign-ins", "private", "local-state", "tui", "fleet", "live", "desktop", "laptop", "lander",
];

const SYNONYMS: [(&str, &str); 3] = [("chrome-profile", "sign-ins"), ("native-client", "native"), ("gpu", "native")];
const CHECKOUT: &str = "checkout:";

static DECLARED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bNeeds:\s*([a-z][\w:.-]*(?:\s*,\s*[a-z][\w:.-]*)*)").expect("declared needs pattern"));
static DONE_WHEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bdone when:?").expect("done when pattern"));
static COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]+)`").expect("box command pattern"));

pub const LANDER: &str = "lander";
const LANDER_LAND: [&str; 7] = ["doctor", "gate-stats", "missed", "profile", "promote", "replay", "serve"];

pub fn words(text: &str, key: &str) -> Vec<String> {
    field(text, key)
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',' || c == '|' || c == '/')
        .filter(|s| !s.is_empty())
        .map(|word| SYNONYMS.iter().find(|(name, _)| *name == word).map_or(word, |(_, need)| need).to_owned())
        .collect()
}

/// `text` led by a `Needs:` line holding its own first `Needs:` words and `extra`: the readers of a brief take the first `Needs:` line.
pub fn with_needs(text: &str, extra: &[String]) -> String {
    if extra.is_empty() {
        return text.to_owned();
    }
    let mut merged = words(text, "Needs");
    for need in extra {
        if !merged.contains(need) {
            merged.push(need.clone());
        }
    }
    format!("Needs: {}\n{text}", merged.join(", "))
}

/// The needs a box line declares for itself with `Needs: a, b` (`none` for a box that needs nothing beyond its repository), which replace what its prose would name.
pub fn declared(line: &str) -> Option<Vec<String>> {
    let listed = DECLARED.captures(line)?[1].to_lowercase();
    let mut needs: Vec<String> = listed.split(',').map(|word| word.trim()).filter(|word| *word != "none").map(|word| SYNONYMS.iter().find(|(name, _)| *name == word).map_or(word, |(_, need)| need).to_owned()).collect();
    needs.sort();
    needs.dedup();
    Some(needs)
}

pub fn implied(line: &str) -> Vec<String> {
    let check = DONE_WHEN.find_iter(line).last().map_or(line, |found| &line[found.end()..]);
    if COMMAND.captures_iter(check).any(|found| lander_state(&found[1])) { vec![LANDER.to_owned()] } else { Vec::new() }
}

fn lander_state(command: &str) -> bool {
    let words: Vec<&str> = command.split(|character: char| character.is_whitespace() || "|&;()<>".contains(character)).filter(|word| !word.is_empty()).collect();
    let land = words.windows(3).any(|run| run[0] == "north" && run[1] == "land" && LANDER_LAND.contains(&run[2]));
    let loops = words.windows(3).any(|run| run[0] == "north" && run[1] == "loops" && run[2] == "check") && words.windows(2).any(|run| run[0] == "--role" && run[1] == "lander");
    land || loops
}

pub(super) fn is_local_need(word: &str) -> bool {
    LOCAL_NEEDS.contains(&word) || word.starts_with(CHECKOUT)
}

/// Whether a host's `serves` list covers `checkout:REPO`: it serves some need for that repository, or a bare need,
/// which it serves for every repository.
pub(super) fn holds_checkout(serves: &[String], need: &str) -> Option<bool> {
    let repo = need.strip_prefix(CHECKOUT)?;
    Some(serves.iter().any(|served| served.split_once(':').is_none_or(|(_, there)| there == repo)))
}
