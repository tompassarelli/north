use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use north_base::platform::process;
use regex::Regex;
use serde_json::Value;

use super::exec::git;

static CITED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Za-z][\w.-]*)#(\d+)\b").unwrap());
const GH_LIMIT: Duration = Duration::from_secs(60);
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

pub(super) fn box_line(branch: &str, tip: &str, age_h: i64) -> String {
    format!(
        "- [ ] Unlanded origin branch `{branch}` ({}, {age_h} h old) is on main by no commit: land it through safe-push, rebuild it, or delete it with `safe-push abandon --superseded {branch} --by COMMIT` naming the commit on main that supersedes it. Done when: `git ls-remote --exit-code origin refs/heads/{branch}; test $? -eq 2` exits 0. (lane gc --remote)",
        &tip[..tip.len().min(12)]
    )
}

pub(super) fn open_box_names(body: &str, branch: &str) -> bool {
    let quoted = format!("`{branch}`");
    body.lines().any(|line| line.trim_start().starts_with("- [ ]") && (line.contains(&quoted) || line.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | ';' | ':')).any(|word| word == branch)))
}

pub(super) fn named_in_open_box(open: &[Value], branch: &str) -> Option<u64> {
    open.iter().find(|issue| issue["body"].as_str().is_some_and(|body| open_box_names(body, branch))).and_then(|issue| issue["number"].as_u64())
}

pub(super) fn cited_open(text: &str, repo: &str, open: &[Value]) -> Option<u64> {
    CITED
        .captures_iter(text)
        .filter(|found| &found[1] == repo)
        .filter_map(|found| found[2].parse::<u64>().ok())
        .find(|number| open.iter().any(|issue| issue["number"].as_u64() == Some(*number)))
}

fn gh(arguments: &[&str], input: Option<&str>) -> Result<String, String> {
    let output = process::run_with_input_within("gh", arguments, input.map(str::as_bytes), GH_LIMIT).ok_or_else(|| format!("gh {} did not finish", arguments[..2].join(" ")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("gh {}: {}", arguments[..2].join(" "), String::from_utf8_lossy(&output.stderr).trim()))
    }
}

pub(super) fn append_box(repo: &str, number: u64, line: &str, branch: &str) -> Result<bool, String> {
    let slug = format!("tompassarelli/{repo}");
    let number = number.to_string();
    let issue: Value = serde_json::from_str(&gh(&["issue", "view", &number, "-R", &slug, "--json", "body,state"], None)?).map_err(|error| error.to_string())?;
    let body = issue["body"].as_str().unwrap_or_default();
    if issue["state"].as_str() != Some("OPEN") || open_box_names(body, branch) {
        return Ok(false);
    }
    gh(&["issue", "edit", &number, "-R", &slug, "--body-file", "-"], Some(&format!("{}\n\n{line}\n", body.trim_end())))?;
    Ok(true)
}

pub(super) fn tell_unboxed(repo: &str, branch: &str, line: &str) -> Result<(), String> {
    let title = format!("Unlanded origin branch {branch} is named in no open box");
    north_base::platform::coordinator_tell::tell_coordinator(&format!("tompassarelli/{repo} {title}"), Some(&format!("{title} on {repo}: {line}")))
}
