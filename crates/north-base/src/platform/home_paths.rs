use std::path::{Component, Path, PathBuf};

/// What a carried report's path reads as once this host no longer holds it.
pub const ABSENT: &str = "[absent on this host]";

const BRACKETS: &str = "\"'`(<[{)]}>,;:.";

/// Every absolute path the prompt names under `home`, with a leading `~/` expanded, that only the launching host holds: a file a session placed on another machine cannot read. A path inside a repository's primary checkout, `~/code/REPO/main`, is no such file, since every fleet host keeps that checkout.
pub fn home_paths(prompt: &str, home: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for word in prompt.split_whitespace() {
        let path = expand(word.trim_matches(|c: char| BRACKETS.contains(c)), home);
        if path != home && path.starts_with(home) && !in_primary_checkout(&path, home) && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// `prompt` with every path under `home` that this host does not hold replaced by [`ABSENT`], keeping whitespace and the word's own punctuation: a report written before a lane was removed must not send the next worker to it.
pub fn without_absent(prompt: &str, home: &Path) -> String {
    let mut out = String::with_capacity(prompt.len());
    for part in prompt.split_inclusive(char::is_whitespace) {
        let word = part.trim_end_matches(char::is_whitespace);
        let core = word.trim_matches(|c: char| BRACKETS.contains(c));
        let path = expand(core, home);
        if !core.is_empty() && path != home && path.starts_with(home) && !path.exists() {
            let lead = word.len() - word.trim_start_matches(|c: char| BRACKETS.contains(c)).len();
            out.push_str(&word[..lead]);
            out.push_str(ABSENT);
            out.push_str(&word[lead + core.len()..]);
        } else {
            out.push_str(word);
        }
        out.push_str(&part[word.len()..]);
    }
    out
}

fn expand(word: &str, home: &Path) -> PathBuf {
    if let Some(rest) = word.strip_prefix("~/").or_else(|| word.strip_prefix("$HOME/")) {
        return home.join(rest);
    }
    word.strip_prefix("$XDG_STATE_HOME/").map_or_else(|| PathBuf::from(word), |rest| home.join(".local/state").join(rest))
}

fn in_primary_checkout(path: &Path, home: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(home) else { return false };
    let mut parts = relative.components();
    matches!((parts.next(), parts.next(), parts.next()), (Some(Component::Normal(code)), Some(Component::Normal(_)), Some(Component::Normal(main))) if code == "code" && main == "main")
        && parts.all(|part| matches!(part, Component::Normal(_)))
}
