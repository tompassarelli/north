//! A lane that retires a `north` verb from the CLI reference or deletes a source path is refused while an open box
//! in the GitHub mirror still names it, so a retirement cannot strand the boxes whose checks run it (north#1054). A box on an issue the lane's own commits name is the lane's to edit or tick, so it does not count.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use north_cli_reference::Verbs;
use north_github_mirror::store::{Mirror, OPEN_ISSUES, key};
use regex::Regex;

use super::git;

const SELF: &str = "stranded-boxes";

struct Retired {
    verbs: Option<(Verbs, Verbs)>,
    paths: BTreeSet<String>,
}

impl Retired {
    fn read(directory: &Path, from: &str, head: &str) -> Self {
        let reference = |revision: &str| git::text(directory, &["show", &format!("{revision}:{}", north_cli_reference::REFERENCE)]).map(|text| Verbs::parse(&text));
        let deleted = git::text(directory, &["diff", "--name-only", "--no-renames", "--diff-filter=D", from, head, "--"]).unwrap_or_default();
        let mut paths = BTreeSet::new();
        for file in deleted.lines() {
            paths.insert(file.to_owned());
            for (at, _) in file.match_indices('/') {
                let parent = &file[..at];
                if git::text(directory, &["cat-file", "-t", &format!("{head}:{parent}")]).is_none() {
                    paths.insert(parent.to_owned());
                }
            }
        }
        Self { verbs: reference(from).zip(reference(head)), paths }
    }

    fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.verbs.as_ref().is_none_or(|(before, after)| before == after)
    }

    fn named_by(&self, text: &str) -> Option<String> {
        let verb = self.verbs.as_ref().and_then(|(before, after)| north_cli_reference::calls(text).iter().find_map(|call| after.lacks(call).filter(|_| before.lacks(call).is_none())));
        verb.or_else(|| {
            text.split(|c: char| c.is_whitespace() || matches!(c, ',' | '`' | '(' | ')' | '\'' | '"'))
                .map(|word| word.trim_end_matches(['.', ';', ':']).trim_end_matches('/'))
                .find(|word| self.paths.contains(*word))
                .map(str::to_owned)
        })
    }
}

/// The issues of `repo` the lane's commits name, whose boxes the lane itself edits or ticks once it lands.
fn worked(messages: &str, repo: &str) -> BTreeSet<u64> {
    let reference = Regex::new(r"(?:\b([A-Za-z0-9_.-]+))?#(\d+)").expect("issue reference pattern");
    reference.captures_iter(messages).filter(|found| found.get(1).is_none_or(|named| named.as_str().eq_ignore_ascii_case(repo))).filter_map(|found| found[2].parse().ok()).collect()
}

/// Writes each open box of the repository's issues that names what `from..head` retires, and answers whether any does.
pub fn scan(directory: &Path, from: &str, head: &str, mirror: &Mirror, out: &mut dyn Write) -> bool {
    let retired = Retired::read(directory, from, head);
    if retired.is_empty() {
        return false;
    }
    let repo = git::origin_name(directory);
    let own = worked(&git::messages(directory, from, head), &repo);
    let Some(issues) = mirror.read(&key(&repo, OPEN_ISSUES)).map(|copy| copy.body) else {
        let _ = writeln!(out, "{SELF}: ⚠ no open {repo} issues in {}, so the verbs and paths this lane retires were not checked against open boxes", mirror.path().display());
        return false;
    };
    let mut stranded = Vec::new();
    for issue in issues.as_array().into_iter().flatten() {
        let number = issue["number"].as_u64().unwrap_or_default();
        if own.contains(&number) {
            continue;
        }
        for text in issue["body"].as_str().unwrap_or_default().lines().filter_map(|line| ["- [ ] ", "* [ ] "].iter().find_map(|marker| line.trim_start().strip_prefix(marker))) {
            if let Some(what) = retired.named_by(text) {
                stranded.push(format!("{repo}#{number}: `{what}` in box: {text}"));
            }
        }
    }
    if stranded.is_empty() {
        return false;
    }
    let _ = writeln!(out, "safe-push: ✗ {SELF}: this lane retires what an open box's check still names; edit or tick each box first:");
    for line in &stranded {
        let _ = writeln!(out, "  {line}");
    }
    true
}

#[cfg(test)]
#[path = "stranded_boxes_tests.rs"]
mod tests;
