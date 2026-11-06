use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::{Range, git};

const POLICY_TEXT: [&str; 3] = [":(glob)**/AGENTS.md", ":(glob)**/SKILL.md", ":(glob)**/skills/**/*.md"];

static RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(must|never)\b").expect("rule pattern"));
static ENFORCED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(hooks?|guards?|gates?|checks?|scans?|lints?|tests?|refus\w*|blocks?|blocked|deny|denies|denied)\b|\bprose-only:?\s*[\w./-]*#\d+").expect("enforced pattern"));

/// The must or never lines `diff` adds that name no enforcing hook, gate or check and carry no
/// `prose-only: REPO#N` owner issue; a line the same diff removes elsewhere was moved, not added.
pub fn unowned(diff: &str) -> Vec<(String, String)> {
    let mut file = String::new();
    let mut added = Vec::new();
    let mut removed = BTreeSet::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            file = path.strip_prefix("b/").unwrap_or(path).to_owned();
        } else if let Some(text) = line.strip_prefix('+') {
            added.push((file.clone(), text.trim().to_owned()));
        } else if let Some(text) = line.strip_prefix('-').filter(|_| !line.starts_with("--- ")) {
            removed.insert(text.trim().to_owned());
        }
    }
    added.into_iter().filter(|(_, text)| RULE.is_match(text) && !ENFORCED.is_match(text) && !removed.contains(text)).collect()
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "rule-owners: expected BASE..HEAD, got {range}");
        return 2;
    };
    let fork = git::merge_base(directory, base, head);
    let mut arguments = vec!["diff", "--unified=0", "--no-color", "--no-ext-diff", "--no-renames", fork.as_str(), head, "--"];
    arguments.extend(POLICY_TEXT);
    let Some(diff) = git::text(directory, &arguments) else {
        let _ = writeln!(out, "rule-owners: cannot diff {fork}..{head}");
        return 1;
    };
    let lines = unowned(&diff);
    if lines.is_empty() {
        return 0;
    }
    let _ = writeln!(out, "rule-owners: refused: each new must or never line in AGENTS.md or a skill names the hook, gate or check that enforces it, or ends with `(prose-only: REPO#N)` naming the issue that owns its guard:");
    for (file, text) in lines {
        let _ = writeln!(out, "  {file}: {text}");
    }
    1
}
