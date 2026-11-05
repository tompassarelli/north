use sha2::{Digest, Sha256};

const SWAPS: &[(&str, &str)] = &[
    (" == ", " != "),
    (" != ", " == "),
    (" <= ", " < "),
    (" >= ", " > "),
    (" < ", " <= "),
    (" > ", " >= "),
    (" && ", " || "),
    (" || ", " && "),
    (" + ", " - "),
    (" - ", " + "),
    ("true", "false"),
    ("false", "true"),
];
const CONTEXT: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Site {
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) from: &'static str,
    pub(super) to: &'static str,
}

impl Site {
    pub(super) fn label(&self) -> String {
        format!("{}:{}:{} {}->{}", self.path, self.line, self.column + 1, self.from.trim(), self.to.trim())
    }
}

fn word_at(text: &str, at: usize, word: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    !text[..at].chars().next_back().is_some_and(ident) && !text[at + word.len()..].chars().next().is_some_and(ident)
}

fn code_end(line: &str) -> usize {
    line.find("//").unwrap_or(line.len())
}

pub(super) fn sites(path: &str, text: &str) -> Vec<Site> {
    if !text.ends_with('\n') {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        let code = &line[..code_end(line)];
        for &(from, to) in SWAPS {
            let alphabetic = from.starts_with(|c: char| c.is_ascii_alphabetic());
            for (column, _) in code.match_indices(from) {
                if !alphabetic || word_at(code, column, from) {
                    found.push(Site { path: path.to_owned(), line: index + 1, column, from, to });
                }
            }
        }
    }
    found
}

pub(super) fn sample(mut all: Vec<Site>, seed: &str, count: usize) -> Vec<Site> {
    let rank = |site: &Site| Sha256::digest(format!("{seed}\0{}", site.label()));
    all.sort_by_cached_key(rank);
    all.truncate(count);
    all
}

pub(super) fn patch(site: &Site, text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let at = site.line - 1;
    let first = at.saturating_sub(CONTEXT);
    let last = (at + CONTEXT).min(lines.len() - 1);
    let original = lines[at];
    let mutated = format!("{}{}{}", &original[..site.column], site.to, &original[site.column + site.from.len()..]);
    let count = last - first + 1;
    let mut out = format!("diff --git a/{0} b/{0}\n--- a/{0}\n+++ b/{0}\n@@ -{1},{2} +{1},{2} @@\n", site.path, first + 1, count);
    for (index, line) in lines.iter().enumerate().take(last + 1).skip(first) {
        if index == at {
            out.push_str(&format!("-{original}\n+{mutated}\n"));
        } else {
            out.push_str(&format!(" {line}\n"));
        }
    }
    out
}
