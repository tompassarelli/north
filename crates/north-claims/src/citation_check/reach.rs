use std::collections::BTreeSet;
use std::path::Path;

use super::BrokenPath;

const ENTRIES: &[&str] = &["README.md", "AGENTS.md"];

fn top_level_doc(path: &str) -> bool {
    path.strip_prefix("docs/").is_some_and(|name| name.ends_with(".md") && !name.contains('/'))
}

pub fn unlinked(root: &Path, files: &[String], known: &BTreeSet<String>) -> Vec<String> {
    let mut linked = BTreeSet::new();
    for entry in ENTRIES {
        let Ok(text) = std::fs::read_to_string(root.join(entry)) else { continue };
        for line in text.lines() {
            linked.extend(super::link_targets(line).into_iter().filter_map(|target| super::resolved(known, "", target)));
        }
    }
    files.iter().filter(|path| top_level_doc(path) && !linked.contains(*path)).cloned().collect()
}

pub fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

pub fn anchors(text: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let Some(heading) = line.strip_prefix('#').map(|rest| rest.trim_start_matches('#')) else { continue };
        if fenced || !heading.starts_with(' ') {
            continue;
        }
        let base = slug(heading);
        let mut anchor = base.clone();
        let mut repeat = 0;
        while !seen.insert(anchor.clone()) {
            repeat += 1;
            anchor = format!("{base}-{repeat}");
        }
    }
    seen
}

pub fn broken_anchors(root: &Path, path: &str, text: &str, known: &BTreeSet<String>) -> Vec<BrokenPath> {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut fenced = false;
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for target in super::link_targets(line) {
            let Some((file, anchor)) = target.split_once('#') else { continue };
            if !file.ends_with(".md") || anchor.is_empty() {
                continue;
            }
            let Some(resolved) = super::resolved(known, dir, file) else { continue };
            let Ok(linked) = std::fs::read_to_string(root.join(&resolved)) else { continue };
            if !anchors(&linked).contains(anchor) {
                found.push(BrokenPath { path: path.into(), line: index + 1, target: target.into() });
            }
        }
    }
    found
}
