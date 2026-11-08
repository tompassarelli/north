use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use regex::Regex;

const MARKER: usize = 31;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Insert {
    Lines(String),
    Tables(Vec<String>),
}

fn merge_file(base: &str, ours: &str, theirs: &str, options: &[String]) -> Result<(String, bool), String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let paths = ["ours", "base", "theirs"].map(|name| dir.path().join(name));
    for (path, text) in paths.iter().zip([ours, base, theirs]) {
        fs::write(path, text).map_err(|error| error.to_string())?;
    }
    let status = Command::new("git")
        .args(["merge-file", "-L", "ours", "-L", "base", "-L", "theirs"])
        .args(options)
        .args(&paths)
        .status()
        .map_err(|error| format!("git merge-file: {error}"))?;
    let code = status.code().ok_or("git merge-file was killed")?;
    if !(0..=127).contains(&code) {
        return Err(format!("git merge-file failed with {code}"));
    }
    Ok((fs::read_to_string(&paths[0]).map_err(|error| error.to_string())?, code > 0))
}

pub(super) fn text_merge(base: &str, ours: &str, theirs: &str) -> Result<(String, bool), String> {
    merge_file(base, ours, theirs, &[])
}

pub(super) fn conflict_text(base: &str, ours: &str, theirs: &str) -> String {
    match text_merge(base, ours, theirs) {
        Ok((text, true)) => text,
        _ => format!("<<<<<<< ours\n{ours}=======\n{theirs}>>>>>>> theirs\n"),
    }
}

pub(super) fn validate(insert: &Insert) -> Result<(), String> {
    match insert {
        Insert::Lines(pattern) => Regex::new(pattern).map(drop).map_err(|error| format!("pattern {pattern:?}: {error}")),
        Insert::Tables(_) => Ok(()),
    }
}

struct Hunk<'a> {
    ours: Vec<&'a str>,
    base: Vec<&'a str>,
    theirs: Vec<&'a str>,
    text: String,
}

enum Part {
    Ours,
    Base,
    Theirs,
}

pub(super) fn resolve(base: &str, ours: &str, theirs: &str, inserts: &[Insert]) -> Result<(String, bool), String> {
    let (merged, conflicts) = merge_file(base, ours, theirs, &["--diff3".to_owned(), format!("--marker-size={MARKER}")])?;
    if !conflicts {
        return Ok((merged, false));
    }
    let (open, middle, close) = (format!("{} ours", "<".repeat(MARKER)), "=".repeat(MARKER), format!("{} theirs", ">".repeat(MARKER)));
    let based = format!("{} base", "|".repeat(MARKER));
    let mut out = String::new();
    let mut left = false;
    let mut hunk: Option<(Hunk, Part)> = None;
    for line in merged.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        let Some((current, part)) = hunk.as_mut() else {
            if bare == open {
                hunk = Some((Hunk { ours: Vec::new(), base: Vec::new(), theirs: Vec::new(), text: line.to_owned() }, Part::Ours));
            } else {
                out.push_str(line);
            }
            continue;
        };
        current.text.push_str(line);
        match part {
            Part::Ours if bare == based => *part = Part::Base,
            Part::Base if bare == middle => *part = Part::Theirs,
            Part::Theirs if bare == close => {
                let Some((done, _)) = hunk.take() else { continue };
                match union(inserts, &done) {
                    Some(lines) if done.base.is_empty() => lines.iter().for_each(|line| {
                        out.push_str(line);
                        out.push('\n');
                    }),
                    _ => {
                        left = true;
                        out.push_str(&done.text);
                    }
                }
            }
            Part::Ours => current.ours.push(bare),
            Part::Base => current.base.push(bare),
            Part::Theirs => current.theirs.push(bare),
        }
    }
    if let Some((open_hunk, _)) = hunk {
        return Err(format!("git merge-file left an unclosed conflict: {}", open_hunk.text.lines().next().unwrap_or_default()));
    }
    Ok((out, left))
}

fn union(inserts: &[Insert], hunk: &Hunk) -> Option<Vec<String>> {
    inserts.iter().find_map(|insert| match insert {
        Insert::Lines(pattern) => lines_union(&Regex::new(pattern).ok()?, &hunk.ours, &hunk.theirs),
        Insert::Tables(names) => tables_union(names, &hunk.ours, &hunk.theirs),
    })
}

fn identities<'a>(pattern: &Regex, lines: &[&'a str]) -> Option<Vec<&'a str>> {
    lines.iter().map(|line| pattern.captures(line).map(|found| found.get(1).map_or(*line, |key| key.as_str()))).collect()
}

fn sorted(lines: &[&str]) -> bool {
    lines.windows(2).all(|pair| pair[0] <= pair[1])
}

fn lines_union(pattern: &Regex, ours: &[&str], theirs: &[&str]) -> Option<Vec<String>> {
    let (mine, other) = (identities(pattern, ours)?, identities(pattern, theirs)?);
    let mut seen = BTreeSet::new();
    if !mine.iter().chain(&other).all(|key| seen.insert(*key)) {
        return None;
    }
    let mut lines = ours.iter().chain(theirs).map(|line| (*line).to_owned()).collect::<Vec<_>>();
    if sorted(ours) && sorted(theirs) {
        lines.sort();
    }
    Some(lines)
}

fn tables<'a>(names: &[String], part: &[&'a str]) -> Option<Vec<Vec<&'a str>>> {
    let mut found: Vec<Vec<&str>> = Vec::new();
    for line in part.iter().copied().filter(|line| !line.trim().is_empty()) {
        let header = line.strip_prefix("[[").and_then(|rest| rest.strip_suffix("]]"));
        match (header, found.last_mut()) {
            (Some(name), _) if names.iter().any(|known| known == name) => found.push(vec![line]),
            (None, Some(table)) if !line.starts_with('[') => table.push(line),
            _ => return None,
        }
    }
    (!found.is_empty()).then_some(found)
}

fn blank_edges(part: &[&str]) -> (usize, usize) {
    let lead = part.iter().take_while(|line| line.trim().is_empty()).count();
    let trail = part.iter().rev().take_while(|line| line.trim().is_empty()).count();
    (lead, trail)
}

fn tables_union(names: &[String], ours: &[&str], theirs: &[&str]) -> Option<Vec<String>> {
    let (mine, other) = (tables(names, ours)?, tables(names, theirs)?);
    if mine.iter().any(|table| other.contains(table)) {
        return None;
    }
    let (lead, trail) = blank_edges(ours);
    let mut lines = vec![String::new(); lead];
    for (at, table) in mine.iter().chain(&other).enumerate() {
        if at > 0 {
            lines.push(String::new());
        }
        lines.extend(table.iter().map(|line| (*line).to_owned()));
    }
    lines.extend(std::iter::repeat_n(String::new(), trail));
    Some(lines)
}
