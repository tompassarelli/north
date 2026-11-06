use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

static CREATES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\s*(?:[-*]\s*)?creates:.*$").expect("creates pattern"));
static BACKTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`\s]+)`").expect("backtick pattern"));
static LOCATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?::\d+(?:-\d+)?|::.*)$").expect("location suffix pattern"));
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]\(<?([^)\s>]+)").expect("markdown link pattern"));

pub(super) struct Tree {
    pub(super) files: BTreeSet<String>,
    tops: BTreeSet<String>,
}

impl Tree {
    pub(super) fn new(listing: &str) -> Self {
        let files: BTreeSet<String> = listing.lines().map(str::to_owned).collect();
        let tops = files.iter().filter_map(|path| path.split('/').next()).map(str::to_owned).collect();
        Self { files, tops }
    }

    pub(super) fn contains(&self, path: &str) -> bool {
        let directory = format!("{}/", path.trim_end_matches('/'));
        self.files.contains(path) || self.files.range(directory.clone()..).next().is_some_and(|next| next.starts_with(&directory))
    }
}

pub(super) fn missing_paths(text: &str, tree: &Tree) -> Vec<String> {
    let text = CREATES.replace_all(text, "");
    let mut missing: Vec<String> = BACKTICKED
        .captures_iter(&text)
        .map(|found| LOCATION.replace(found[1].trim_end_matches(['.', ',', ';']), "").into_owned())
        .filter(|token| {
            let trailing = token.ends_with('/');
            let last = token.rsplit('/').find(|part| !part.is_empty()).unwrap_or_default();
            let plain = token.chars().all(|character| character.is_ascii_alphanumeric() || "/._-@".contains(character)) && !token.starts_with(['/', '.']);
            plain && token.contains('/') && (trailing || last.contains('.')) && token.split('/').next().is_some_and(|top| tree.tops.contains(top))
        })
        .filter(|token| !tree.contains(token))
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

pub(super) fn broken_links(document: &str, text: &str, tree: &Tree) -> Vec<String> {
    let folder = document.rsplit_once('/').map_or("", |(folder, _)| folder);
    let mut broken: Vec<String> = LINK
        .captures_iter(text)
        .filter_map(|found| {
            let target = found[1].split(['#', '?']).next().unwrap_or_default();
            if target.is_empty() || target.starts_with('/') || target.contains(':') {
                return None;
            }
            let mut parts: Vec<&str> = folder.split('/').filter(|part| !part.is_empty()).collect();
            for part in target.split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        parts.pop()?;
                    }
                    other => parts.push(other),
                }
            }
            let resolved = parts.join("/");
            (!resolved.is_empty() && !tree.contains(&resolved)).then(|| found[1].split(['#', '?']).next().unwrap_or_default().to_owned())
        })
        .collect();
    broken.sort();
    broken.dedup();
    broken
}
