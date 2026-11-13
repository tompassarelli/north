use super::bar::{Bar, NUMBER, UNIT, bars, quantity, word};
use super::git::git;
use super::record::{fields, origin, records};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

fn gating(path: &str, line: &str) -> bool {
    re!(r"(^|/)PLAN[^/]*\.md$").is_match(path) && line.trim_start().starts_with('|')
        || line.contains("Done when")
        || re!(r"^\s*[-*] \[ \]").is_match(line)
}
pub(super) fn new_bars(repo: &Path, base: &str, head: &str) -> Result<Vec<(String, Bar, String)>, String> {
    let base = git(repo, &["merge-base", base, head])
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| base.into());
    let mut added = Vec::new();
    let mut removed = BTreeSet::new();
    let mut path = "";
    let mut header = false;
    let diff = git(
        repo,
        &[
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            &base,
            head,
        ],
    )?;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            header = true;
        } else if header && line.starts_with("--- ") {
            if let Some(p) = line.strip_prefix("--- a/") {
                path = p;
            }
        } else if header && line.starts_with("+++ ") {
            if let Some(p) = line.strip_prefix("+++ b/") {
                path = p;
            }
            header = false;
        } else if !header && line.starts_with('-') {
            for bar in bars(&line[1..]) {
                removed.insert((path.to_owned(), bar.comparator, bar.number, bar.unit));
            }
        } else if !header && line.starts_with('+') && gating(path, &line[1..]) {
            for bar in bars(&line[1..]) {
                added.push((path.to_owned(), bar, line[1..].trim().to_owned()));
            }
        }
    }
    let mut covered = BTreeSet::new();
    let log = git(repo, &["log", "--format=%B", &format!("{base}..{head}")])?;
    static QUANTITIES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(r"{NUMBER}\s*(?:[×x]\s+(?i:fewer)\s+)?{UNIT}")).unwrap()
    });
    let quantities = &*QUANTITIES;
    for value in records(&log, "Provenance:") {
        let (claim, named, soft) = fields(value);
        if named.contains_key("rationale")
            && (origin(named.get("origin").copied().unwrap_or(""))
                || soft && named.get("origin") == Some(&"none"))
        {
            for m in quantities.captures_iter(claim) {
                let m0 = m.get(0).unwrap();
                if claim[..m0.start()]
                    .chars()
                    .next_back()
                    .is_some_and(|c| word(c) || c == '.')
                {
                    continue;
                }
                let end = m.get(2).unwrap().end();
                if claim[end..].chars().next().is_some_and(word) {
                    continue;
                }
                covered.insert(quantity(&m[1], &m[2]));
            }
        }
    }
    let mut seen = BTreeSet::new();
    Ok(added
        .into_iter()
        .filter(|(p, b, l)| {
            !removed.contains(&(
                p.clone(),
                b.comparator.clone(),
                b.number.clone(),
                b.unit.clone(),
            )) && !covered.contains(&(b.number.clone(), b.unit.clone()))
                && seen.insert((p.clone(), b.clone(), l.clone()))
        })
        .collect())
}
