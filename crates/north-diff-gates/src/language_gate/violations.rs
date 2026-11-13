use super::allowlist::{Allowlist, Class, Entry, classify, covered_by};
use super::{ALLOWLIST, ALTERNATIVE, EMPTY_TREE, LIBRARY_GAP, Result};
use crate::platform::git as platform_git;
use std::path::{Component, Path};

pub(super) fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let literal: Vec<&str> = ["--literal-pathspecs"].into_iter().chain(args.iter().copied()).collect();
    platform_git::stdout(repo, &literal, |output| {
        format!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })
}

pub(super) fn blob(repo: &Path, rev: &str, path: &str) -> Result<Option<Vec<u8>>> {
    let tree = git(repo, &["ls-tree", "-z", rev, "--", path])?;
    if tree.is_empty() {
        return Ok(None);
    }
    let object = format!("{rev}:{path}");
    let kind = git(repo, &["cat-file", "-t", &object])?;
    if kind != b"blob\n" {
        return Ok(None);
    }
    git(repo, &["show", &object]).map(Some)
}

pub(super) fn revision_allowlist(repo: &Path, rev: &str) -> Result<Allowlist> {
    blob(repo, rev, ALLOWLIST)?
        .map(|bytes| Allowlist::parse(&bytes))
        .unwrap_or_else(|| Ok(Allowlist::default()))
}

pub(super) fn line_count(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte == b'\n').count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"))
}

pub(super) fn shell_code_lines(bytes: &[u8]) -> usize {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| {
            let line = line.trim_ascii_start();
            !line.is_empty() && !line.starts_with(b"#")
        })
        .count()
}

pub(super) fn has_approval(repo: &Path, base: &str, head: &str) -> Result<bool> {
    let log = git(
        repo,
        &[
            "log",
            "--format=%(trailers:key=Language-allowlist,valueonly)",
            &format!("{base}..{head}"),
        ],
    )?;
    Ok(String::from_utf8_lossy(&log).lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("Tom")
            && words.next().is_some_and(|date| !date.is_empty())
            && words.next().is_some()
    }))
}

pub(super) fn calendar_date(value: &str) -> Option<time::Date> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return None;
    }
    time::Date::from_calendar_date(
        value[..4].parse().ok()?,
        time::Month::try_from(value[5..7].parse::<u8>().ok()?).ok()?,
        value[8..].parse().ok()?,
    )
    .ok()
}

pub(super) fn python_entry_problem(
    repo: &Path,
    head: &str,
    date: time::Date,
    entry: &Entry,
) -> Result<Option<&'static str>> {
    for (field, value) in [
        ("port_owner", &entry.port_owner),
        ("port_target", &entry.port_target),
        ("commitment", &entry.commitment),
        ("created", &entry.created),
    ] {
        if value.as_ref().is_none_or(|value| value.trim().is_empty()) {
            return Ok(Some(field));
        }
    }
    let target = entry.port_target.as_deref().unwrap();
    if !target.ends_with(".rs")
        || target.contains('\\')
        || target
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !Path::new(target)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Ok(Some("port_target (must be a repo-relative Rust path)"));
    }
    if blob(repo, head, target)?.is_some() {
        return Ok(Some("port_target (expired: Rust port already landed)"));
    }
    let Some(created) = entry.created.as_deref().and_then(calendar_date) else {
        return Ok(Some("created (must be YYYY-MM-DD)"));
    };
    if (date - created).whole_days() > 7 {
        return Ok(Some(
            "created (expired: more than 7 days before head commit)",
        ));
    }
    Ok(None)
}

pub(super) fn violations(repo: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let base = if base == EMPTY_TREE {
        EMPTY_TREE.to_string()
    } else {
        String::from_utf8(git(
            repo,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )?)
        .map_err(|error| error.to_string())?
    };
    let head = String::from_utf8(git(
        repo,
        &["rev-parse", "--verify", &format!("{head}^{{commit}}")],
    )?)
    .map_err(|error| error.to_string())?;
    let (base, head) = (base.trim(), head.trim());
    let old_list = revision_allowlist(repo, base)?;
    let list = revision_allowlist(repo, head)?;
    let mut failures = Vec::new();
    if list
        .entries
        .iter()
        .any(|entry| entry.class == Class::Python)
    {
        let date = git(repo, &["show", "-s", "--format=%cs", head])?;
        let date = calendar_date(String::from_utf8_lossy(&date).trim())
            .ok_or("invalid head committer date")?;
        for entry in list
            .entries
            .iter()
            .filter(|entry| entry.class == Class::Python && entry.gap.trim() != LIBRARY_GAP)
        {
            if let Some(field) = python_entry_problem(repo, head, date, entry)? {
                failures.push(format!("{ALLOWLIST}: R4 Python entry {}: missing, invalid or expired {field}; remove the entry or supply a valid Rust port commitment; {ALTERNATIVE}", entry.glob));
            }
        }
    }
    if (old_list.ratchet.contains(&Class::Script) && !list.ratchet.contains(&Class::Script)
        || list
        .entries
        .iter()
        .any(|entry| entry.class != Class::Python && !covered_by(entry, &old_list)))
        && !has_approval(repo, base, head)?
    {
        failures.push(format!("{ALLOWLIST}: R3 allowlist gained or widened an entry or removed the script ratchet without Language-allowlist: Tom <date> <quote>; {ALTERNATIVE}"));
    }
    let diff = git(
        repo,
        &[
            "diff",
            "--name-status",
            "-z",
            "-M",
            "-C",
            "--find-copies-harder",
            base,
            head,
            "--",
        ],
    )?;
    let mut fields = diff
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let status = status[0];
        let first = fields.next().ok_or("missing git diff path")?;
        let (old_path, new_path) = if matches!(status, b'R' | b'C') {
            (first, fields.next().ok_or("missing git diff destination")?)
        } else {
            (first, first)
        };
        if status == b'D' {
            continue;
        }
        let old_path = std::str::from_utf8(old_path).map_err(|error| error.to_string())?;
        let path = std::str::from_utf8(new_path).map_err(|error| error.to_string())?;
        let Some(new) = blob(repo, head, path)? else {
            continue;
        };
        let Some(class) = classify(Path::new(path), &new) else {
            continue;
        };
        let old = if matches!(status, b'A' | b'C') {
            None
        } else {
            blob(repo, base, old_path)?
        };
        let old_class = old
            .as_ref()
            .and_then(|content| classify(Path::new(old_path), content));
        let new_python = class == Class::Python
            && (matches!(status, b'A' | b'C')
                || (status == b'R' && old_class != Some(Class::Python)));
        if (class == Class::Script && !list.ratchet.contains(&Class::Script))
            || (!new_python && list.allows(path, class))
        {
            continue;
        }
        let rule = if class == Class::Shell {
            let before = old.as_ref().filter(|_| old_class == Some(Class::Shell))
                .map_or(0, |bytes| shell_code_lines(bytes));
            let after = shell_code_lines(&new);
            (after > 80 && after > before)
                .then_some("R2 Shell code-line count grew past 80 without an allowlist entry")
        } else if new_python
            || status == b'A'
            || (matches!(status, b'R' | b'C') && old_class != Some(class))
        {
            Some("R1 new Python path is forbidden or Script path is not allowlisted")
        } else if line_count(&new) > old.as_ref().map_or(0, |bytes| line_count(bytes)) {
            Some("R2 Python or Script line count increased without an allowlist entry")
        } else {
            None
        };
        if let Some(rule) = rule {
            failures.push(format!("{path}: {rule}; {ALTERNATIVE}"));
        }
    }
    Ok(failures)
}
