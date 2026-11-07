//! The words a brief's facet line names, and which of them are local needs that keep a box off
//! code-only targets. A chrome-profile is the laptop's sign-ins; native-client and gpu are native work.
//! A box's own text names needs too (`in_box`): the laptop or its desktop, a host by name, every host,
//! or another repository's checkout (`checkout:REPO`), so placement reads them before a session spawns.
use super::field;
use regex::Regex;
use std::sync::LazyLock;

const LOCAL_NEEDS: [&str; 12] = [
    "warcraft", "wc3", "client", "native", "sign-ins", "private", "local-state", "tui", "fleet", "live", "desktop", "laptop",
];

const SYNONYMS: [(&str, &str); 3] = [("chrome-profile", "sign-ins"), ("native-client", "native"), ("gpu", "native")];
const CHECKOUT: &str = "checkout:";

static LAPTOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:on (?:tom's |the )?laptop|laptop's|laptop-placed|(?:the )?laptop (?:rebuild|build|timer|service|tick|runtime|restart)|niri|ghostty)\b").expect("laptop pattern"));
static FLEET_TICK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:through|in|on) the fleet tick\b|\bfleet tick's\b").expect("fleet tick pattern"));
static HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:on|from) ([a-z][\w-]*)\b|\b([a-z][\w-]*)'s\b").expect("host pattern"));
static EVERY_HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:all|every|each)(?: (?:two|three|four|five|six))? hosts?\b").expect("every-host pattern"));
static REPOSITORY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b([a-z][\w-]*)'s\b|\bcode/([a-z][\w-]*)|\bbun ([a-z][\w-]*)\b").expect("repository pattern"));

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

pub(super) fn is_local_need(word: &str) -> bool {
    LOCAL_NEEDS.contains(&word) || word.starts_with(CHECKOUT)
}

/// Whether a host's `serves` list covers `checkout:REPO`: it serves some need for that repository, or a bare need,
/// which it serves for every repository.
pub(super) fn holds_checkout(serves: &[String], need: &str) -> Option<bool> {
    let repo = need.strip_prefix(CHECKOUT)?;
    Some(serves.iter().any(|served| served.split_once(':').is_none_or(|(_, there)| there == repo)))
}

/// The needs a box's text names beyond its own repository `repo`: `laptop` (the laptop, its rebuild, timer or runtime, its desktop), `fleet` (the fleet tick, which the laptop runs), each of `hosts` it names or all of them
/// when it names every host, and `checkout:REPO` for each other repository of `repos` it works in.
pub fn in_box(text: &str, repo: &str, hosts: &[String], repos: &[String]) -> Vec<String> {
    let lowered = text.to_lowercase();
    let mut found: Vec<String> = Vec::new();
    if LAPTOP.is_match(text) {
        found.push("laptop".into());
    }
    if FLEET_TICK.is_match(text) {
        found.push("fleet".into());
    }
    if EVERY_HOST.is_match(text) {
        found.extend(hosts.iter().cloned());
    }
    let named = |pattern: &Regex| -> Vec<String> { pattern.captures_iter(&lowered).filter_map(|found| found.iter().skip(1).flatten().next().map(|word| word.as_str().to_owned())).collect() };
    found.extend(named(&HOST).into_iter().filter(|word| hosts.contains(word)));
    found.extend(named(&REPOSITORY).into_iter().filter(|word| word != repo && repos.contains(word)).map(|word| format!("{CHECKOUT}{word}")));
    found.sort();
    found.dedup();
    found
}
