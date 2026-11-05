use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::land::scans::git;

pub fn triggered(tree: &Path, workflow: &str, changed: &[String]) -> bool {
    if workflow.starts_with("local:") {
        return true;
    }
    let Some(filters) = push_paths(tree, workflow) else { return true };
    let filters: Vec<&str> = filters.lines().filter(|line| !line.is_empty()).collect();
    if filters.is_empty() || filters.iter().any(|pattern| pattern.starts_with('!')) {
        return true;
    }
    filters.iter().any(|pattern| changed.iter().any(|path| matches(pattern.as_bytes(), path.as_bytes())))
}

fn push_paths(tree: &Path, workflow: &str) -> Option<String> {
    let definition = git::bytes(tree, &["show", &format!("refs/remotes/origin/main:.github/workflows/{workflow}")])?;
    let mut yq = Command::new("yq")
        .args(["-r", r#".on.push | select(type == "!!map") | .paths // [] | .[]"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    yq.stdin.take()?.write_all(&definition).ok()?;
    let output = yq.wait_with_output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn matches(pattern: &[u8], path: &[u8]) -> bool {
    let (mut at, mut offset) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while offset < path.len() {
        if pattern.get(at) == Some(&b'*') {
            at += 1;
            star = Some((at, offset));
        } else if let Some(width) = single(&pattern[at..], path[offset]) {
            at += width;
            offset += 1;
        } else if let Some((after, from)) = star {
            at = after;
            offset = from + 1;
            star = Some((after, offset));
        } else {
            return false;
        }
    }
    pattern[at..].iter().all(|byte| *byte == b'*')
}

fn single(pattern: &[u8], byte: u8) -> Option<usize> {
    match pattern {
        [] => None,
        [b'?', ..] => Some(1),
        [b'\\', escaped, ..] => (*escaped == byte).then_some(2),
        [b'[', rest @ ..] => match class(rest, byte) {
            Some((hit, width)) => hit.then_some(width + 1),
            None => (byte == b'[').then_some(1),
        },
        [literal, ..] => (*literal == byte).then_some(1),
    }
}

fn class(rest: &[u8], byte: u8) -> Option<(bool, usize)> {
    let start = usize::from(matches!(rest.first(), Some(b'!' | b'^')));
    let close = rest.iter().skip(start + 1).position(|candidate| *candidate == b']')? + start + 1;
    let set = &rest[start..close];
    let mut hit = false;
    let mut index = 0;
    while index < set.len() {
        if index + 2 < set.len() && set[index + 1] == b'-' {
            hit |= (set[index]..=set[index + 2]).contains(&byte);
            index += 3;
        } else {
            hit |= set[index] == byte;
            index += 1;
        }
    }
    Some((hit != (start == 1), close + 1))
}

#[cfg(test)]
#[path = "workflow_paths_tests.rs"]
mod tests;
