use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::exec::git;

const SEP: char = '\u{1f}';

pub(super) fn superseding(main: &Path, base: &str, tip: &str) -> Result<Option<Vec<String>>, String> {
    let unavailable = || "supersession evidence unavailable".to_string();
    let fork = git(main, &["merge-base", tip, base]);
    let lane = git(main, &["rev-list", "--no-merges", "--format=%at %ae%x1f%s", "--no-commit-header", tip, &format!("^{base}")]);
    if !fork.ok || !lane.ok {
        return Err(unavailable());
    }
    let since = git(main, &["log", "--no-merges", "--format=%h %at %ae%x1f%s", &format!("{}..{base}", fork.out.trim())]);
    if !since.ok {
        return Err(unavailable());
    }
    let landed: Vec<(&str, &str, &str)> = since.out.lines().filter_map(|line| {
        let (sha, rest) = line.split_once(' ')?;
        let (author, subject) = rest.split_once(SEP)?;
        Some((sha, author, subject))
    }).collect();
    let mut named = Vec::new();
    for line in lane.out.lines().filter(|line| !line.is_empty()) {
        let Some((author, subject)) = line.split_once(SEP) else { return Ok(None) };
        let Some((sha, ..)) = landed.iter().find(|(_, landed_author, landed_subject)| *landed_subject == subject || *landed_author == author) else {
            return Ok(None);
        };
        if !named.contains(&sha.to_string()) {
            named.push(sha.to_string());
        }
    }
    if named.is_empty() || !added_lines_on(main, base, fork.out.trim(), tip)? {
        return Ok(None);
    }
    Ok(Some(named))
}

pub(super) fn archived_as(main: &Path, tip: &str) -> Option<String> {
    let archives = git(main, &["for-each-ref", "--format=%(refname)", "refs/archive/"]);
    if !archives.ok {
        return None;
    }
    archives.out.lines().find(|archive| git(main, &["merge-base", "--is-ancestor", tip, archive]).ok).map(str::to_owned)
}

fn added_lines_on(main: &Path, base: &str, fork: &str, tip: &str) -> Result<bool, String> {
    let diff = git(main, &["diff", "--no-renames", "--no-ext-diff", "--no-color", "--unified=0", &format!("{fork}..{tip}")]);
    if !diff.ok {
        return Err("supersession evidence unavailable".into());
    }
    let (mut header, mut file) = (false, None::<&str>);
    let mut present: HashMap<&str, HashSet<String>> = HashMap::new();
    for line in diff.out.lines() {
        if line.starts_with("diff --git ") {
            (header, file) = (true, None);
        } else if header && line.starts_with("@@") {
            header = false;
        } else if header {
            if let Some(path) = line.strip_prefix("+++ b/") {
                file = Some(path);
            } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                return Ok(false);
            }
        } else if let (Some(added), Some(path)) = (line.strip_prefix('+'), file) {
            if !present.contains_key(path) {
                let shown = git(main, &["show", &format!("{base}:{path}")]);
                present.insert(path, if shown.ok { shown.out.lines().map(str::to_owned).collect() } else { HashSet::new() });
            }
            if !present[path].contains(added) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
