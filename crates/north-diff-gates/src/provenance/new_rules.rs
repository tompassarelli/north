use super::git::git;
use super::record::{fields, records, valid_record};
use std::collections::BTreeSet;
use std::path::Path;

fn rule_text(line: &str) -> String {
    re!(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)")
        .replace(line, "")
        .replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn policy_name(path: &str) -> bool {
    matches!(
        Path::new(path).file_name().and_then(|s| s.to_str()),
        Some("AGENTS.md" | "SKILL.md" | "catalog-config.json")
    )
}
fn policy_line(path: &str, line: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    matches!(name, "AGENTS.md" | "SKILL.md")
        && re!(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)").is_match(line)
        || name == "catalog-config.json" && re!(r#""kind"\s*:\s*"hook""#).is_match(line)
}
fn rule_items(path: &str, text: &str) -> Vec<(usize, usize, String)> {
    let mut items: Vec<(usize, usize, String)> = Vec::new();
    let mut fenced = false;
    let mut routers = 0;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if Path::new(path)
            .file_name()
            .is_some_and(|n| n == "AGENTS.md")
            && let Some(h) = re!(r"^(#{1,6})\s+(.+)").captures(line) {
                if routers > 0 && h[1].len() <= routers {
                    routers = 0;
                }
                if h[2].trim() == "Routers" {
                    routers = h[1].len();
                }
            }
        if routers > 0 {
            continue;
        }
        if re!(r"^\s*(?:[-*+]\s+|\d+[.)]\s+)").is_match(line) || policy_line(path, line) {
            items.push((number, number, line.trim().into()));
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty()
            && let Some(last) = items.last_mut()
                && last.1 == number - 1 {
                    last.1 = number;
                    last.2.push(' ');
                    last.2.push_str(line.trim());
                }
    }
    items
}
fn without_paths(text: &str) -> String {
    re!(r"`[^\s`]*(?:/[^\s`]*|\.[A-Za-z][^\s`]*)`|\b[^\s`()]*[/][^\s`()]+")
        .replace_all(text, "")
        .into_owned()
}
fn words(text: &str) -> BTreeSet<String> {
    re!(r"\w+")
        .find_iter(
            &re!(r"`[^`]*`")
                .replace_all(&without_paths(text), "")
                .to_lowercase(),
        )
        .map(|m| m.as_str().into())
        .collect()
}
pub(super) fn new_rules(repo: &Path, base: &str, head: &str) -> Result<Vec<(String, String, String)>, String> {
    let base = git(repo, &["merge-base", base, head])?.trim().to_owned();
    let log = git(repo, &["log", "--format=%B", &format!("{base}..{head}")])?;
    let claims: Vec<_> = records(&log, "Provenance:")
        .into_iter()
        .filter(|v| valid_record(v))
        .map(|v| rule_text(fields(v).0))
        .collect();
    let names = git(
        repo,
        &["diff", "--name-status", "-z", "--find-renames", &base, head],
    )?;
    let mut names = names.split_terminator('\0');
    let mut added = Vec::new();
    let mut removed = Vec::new();
    while let Some(status) = names.next() {
        let name = names.next().ok_or("missing diff path")?;
        let (mut old, mut path) = if status.starts_with(['R', 'C']) {
            (name, names.next().ok_or("missing renamed path")?)
        } else {
            (name, name)
        };
        if status == "A" {
            old = "";
            if path.starts_with("harness/hooks/")
                && !["/lib/", "/fixtures/", ".test."]
                    .iter()
                    .any(|s| path.contains(s))
            {
                added.push((path.to_owned(), "new hook".into(), path.to_owned()));
            }
        } else if status == "D" {
            path = "";
        }
        if !policy_name(old) && !policy_name(path) {
            continue;
        }
        let mut args = vec![
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            &base,
            head,
            "--",
        ];
        if !old.is_empty() {
            args.push(old);
        }
        if !path.is_empty() && path != old {
            args.push(path);
        }
        let mut deleted = BTreeSet::new();
        let mut inserted = BTreeSet::new();
        let mut hunk = None;
        for line in git(repo, &args)?.lines() {
            if let Some(m) = re!(r"^@@ -(\d+)(?:,\d+)? \+(\d+)").captures(line) {
                hunk = Some((
                    m[1].parse::<usize>().unwrap(),
                    m[2].parse::<usize>().unwrap(),
                ));
            } else if let Some((o, n)) = hunk.as_mut() {
                if line.starts_with('-') {
                    deleted.insert(*o);
                    *o += 1;
                } else if line.starts_with('+') {
                    inserted.insert(*n);
                    *n += 1;
                }
            }
        }
        if !old.is_empty() && !deleted.is_empty() && policy_name(old) {
            for (start, end, text) in
                rule_items(old, &git(repo, &["show", &format!("{base}:{old}")])?)
            {
                if (start..=end).any(|n| deleted.contains(&n)) {
                    removed.push(text);
                }
            }
        }
        if !path.is_empty() && !inserted.is_empty() && policy_name(path) {
            for (start, end, text) in
                rule_items(path, &git(repo, &["show", &format!("{head}:{path}")])?)
            {
                if (start..=end).any(|n| inserted.contains(&n)) && policy_line(path, &text) {
                    added.push((path.into(), "new policy line".into(), text));
                }
            }
        }
    }
    let corpus: BTreeSet<_> = removed.iter().flat_map(|t| words(t)).collect();
    let protected = re!(
        r"(?i)\d+(?:\.\d+)?[a-z]*|[<>≤≥]=?|\b(?:one|two|three|four|five|six|seven|eight|nine|ten|few|hundred|thousand|million|billion|never|always|only|must|may|should|not|no|unless|require|refuse|default|under|over|above|below|within|least|most|less|more|fewer|before|after)\b"
    );
    let mut fresh = Vec::new();
    for (path, kind, text) in added {
        let current = rule_text(&text);
        if claims
            .iter()
            .any(|c| c.contains(&path) || c.contains(&current))
        {
            continue;
        }
        if kind == "new hook" {
            fresh.push((path, kind, text));
            continue;
        }
        let found = removed.iter().position(|previous| {
            if current == rule_text(previous) {
                return true;
            }
            let before = without_paths(previous);
            let after = without_paths(&text);
            let b = before.to_lowercase();
            let a = after.to_lowercase();
            if protected
                .find_iter(&b)
                .map(|m| m.as_str())
                .collect::<Vec<_>>()
                != protected
                    .find_iter(&a)
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
            {
                return false;
            }
            let old_words = words(&before);
            let new_words = words(&after);
            if (old_words.contains("default") || new_words.contains("default")) && before != after {
                return false;
            }
            old_words == new_words
                || !old_words.is_empty()
                    && !new_words.is_empty()
                    && old_words.intersection(&new_words).count() as f64
                        / old_words.len().min(new_words.len()) as f64
                        >= 0.85
                    && new_words.difference(&corpus).count() <= 1
        });
        if let Some(i) = found {
            removed.remove(i);
        } else {
            fresh.push((path, kind, text));
        }
    }
    let mut seen = BTreeSet::new();
    Ok(fresh
        .into_iter()
        .filter(|r| seen.insert(r.clone()))
        .collect())
}
