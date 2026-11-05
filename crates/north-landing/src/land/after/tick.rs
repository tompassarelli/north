use std::fmt;
use std::path::Path;
use std::process::Command;

use crate::issue_body::{self, Edit, Issue};
use crate::issue_box_lint;

const OBSERVATION_MARKERS: [&str; 2] = ["(observed", "(headless run)"];
const SHORT_SHA: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Ticked(String),
    NoMatch,
    Ambiguous(usize),
    AlreadyTicked,
    NeedsObservation,
    OpenChildren(usize),
    NoBoxes,
}

impl fmt::Display for Tick {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tick::Ticked(_) => write!(formatter, "ticked"),
            Tick::NoMatch => write!(formatter, "no box matches"),
            Tick::Ambiguous(count) => write!(formatter, "{count} unticked boxes match; quote more of the box"),
            Tick::AlreadyTicked => write!(formatter, "the box is already ticked"),
            Tick::NeedsObservation => write!(formatter, "landed, box not ticked: needs observation"),
            Tick::OpenChildren(count) => write!(formatter, "landed, box not ticked: {count} boxes under it are unticked"),
            Tick::NoBoxes => write!(formatter, "the issue has no checkboxes"),
        }
    }
}

struct Checkbox<'a> {
    line: usize,
    indent: usize,
    ticked: bool,
    text: &'a str,
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn checkbox(line: &str) -> Option<(bool, &str)> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* "))?;
    if let Some(text) = rest.strip_prefix("[ ]") {
        return Some((false, text.trim()));
    }
    rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]")).map(|text| (true, text.trim()))
}

fn checkboxes<'a>(lines: &[&'a str]) -> Vec<Checkbox<'a>> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(line, text)| checkbox(text).map(|(ticked, text)| (line, ticked, text)))
        .map(|(line, ticked, text)| Checkbox { line, indent: indent(lines[line]), ticked, text })
        .collect()
}

pub fn needs_observation(text: &str) -> bool {
    OBSERVATION_MARKERS.iter().any(|marker| text.contains(marker)) || issue_box_lint::hands_tom_a_check(text)
}

fn children(lines: &[&str], boxes: &[Checkbox], parent: usize) -> Vec<usize> {
    let parent = &boxes[parent];
    let end = (parent.line + 1..lines.len()).find(|&line| !lines[line].trim().is_empty() && indent(lines[line]) <= parent.indent).unwrap_or(lines.len());
    boxes.iter().enumerate().filter(|(_, child)| child.line > parent.line && child.line < end).map(|(index, _)| index).collect()
}

fn parent(lines: &[&str], boxes: &[Checkbox], child: usize) -> Option<usize> {
    let child = &boxes[child];
    let line = (0..child.line).rev().find(|&line| !lines[line].trim().is_empty() && indent(lines[line]) < child.indent)?;
    boxes.iter().position(|candidate| candidate.line == line)
}

fn marked(line: &str, sha: Option<&str>) -> String {
    let ticked = line.replacen("[ ]", "[x]", 1);
    let Some(sha) = sha else { return ticked };
    let (text, ending) = match ticked.strip_suffix("\r\n") {
        Some(text) => (text, "\r\n"),
        None => ticked.strip_suffix('\n').map_or((ticked.as_str(), ""), |text| (text, "\n")),
    };
    format!("{text} (landed {}){ending}", &sha[..sha.len().min(SHORT_SHA)])
}

pub fn tick(body: &str, box_text: &str, sha: Option<&str>) -> Tick {
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let boxes = checkboxes(&lines);
    if boxes.is_empty() {
        return Tick::NoBoxes;
    }
    let wanted = box_text.trim();
    if wanted.is_empty() {
        return Tick::NoMatch;
    }
    let open: Vec<usize> = boxes.iter().enumerate().filter(|(_, found)| !found.ticked && found.text.contains(wanted)).map(|(index, _)| index).collect();
    let found = match open.as_slice() {
        [found] => *found,
        [] if boxes.iter().any(|found| found.ticked && found.text.contains(wanted)) => return Tick::AlreadyTicked,
        [] => return Tick::NoMatch,
        many => return Tick::Ambiguous(many.len()),
    };
    if needs_observation(boxes[found].text) {
        return Tick::NeedsObservation;
    }
    let open_children = children(&lines, &boxes, found).into_iter().filter(|&child| !boxes[child].ticked).count();
    if open_children > 0 {
        return Tick::OpenChildren(open_children);
    }
    let mut ticked: Vec<bool> = boxes.iter().map(|found| found.ticked).collect();
    ticked[found] = true;
    let mut cascade = vec![found];
    let mut child = found;
    while let Some(up) = parent(&lines, &boxes, child) {
        if ticked[up] || needs_observation(boxes[up].text) || !children(&lines, &boxes, up).iter().all(|&below| ticked[below]) {
            break;
        }
        ticked[up] = true;
        cascade.push(up);
        child = up;
    }
    let mut edited = String::with_capacity(body.len() + 32);
    for (index, line) in lines.iter().enumerate() {
        match cascade.iter().find(|&&changed| boxes[changed].line == index) {
            Some(&changed) => edited.push_str(&marked(line, (changed == found).then_some(sha).flatten())),
            None => edited.push_str(line),
        }
    }
    Tick::Ticked(edited)
}

pub fn closable(body: &str) -> bool {
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let boxes = checkboxes(&lines);
    !boxes.is_empty() && boxes.iter().all(|found| found.ticked)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Trailer {
    pub item: String,
    pub box_text: String,
}

pub fn trailers(message: &str) -> Result<Vec<Trailer>, String> {
    let mut item: Option<&str> = None;
    let mut found = Vec::new();
    for line in message.lines() {
        if let Some(value) = line.strip_prefix("Item:") {
            item = value.split_whitespace().next();
        } else if let Some(value) = line.strip_prefix("Box:") {
            let box_text = value.trim();
            let Some(item) = item else {
                return Err(format!("Box: {box_text} has no Item: line before it"));
            };
            Issue::parse(item).map_err(|error| format!("Item: {error}"))?;
            if box_text.is_empty() {
                return Err(format!("Item: {item} has an empty Box: line"));
            }
            found.push(Trailer { item: item.to_owned(), box_text: box_text.to_owned() });
        }
    }
    Ok(found)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Landed {
    pub issue: Issue,
    pub box_text: String,
    pub sha: String,
}

pub fn commits(repo: &Path, base: &str, head: &str) -> Result<Vec<(String, String)>, String> {
    let range = format!("{base}..{head}");
    let output = Command::new("git").arg("-C").arg(repo).args(["log", "--reverse", "--format=%H%x1f%B%x1e", &range]).output().map_err(|error| format!("git log {range}: {error}"))?;
    if !output.status.success() {
        return Err(format!("git log {range}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\x1e')
        .filter_map(|record| record.trim_start_matches('\n').split_once('\x1f'))
        .map(|(sha, message)| (sha.to_owned(), message.to_owned()))
        .collect())
}

pub fn landed(repo: &Path, base: &str, head: &str) -> Result<Vec<Landed>, String> {
    let mut found = Vec::new();
    for (sha, message) in commits(repo, base, head)? {
        for trailer in trailers(&message).map_err(|error| format!("{}: {error}", &sha[..sha.len().min(SHORT_SHA)]))? {
            let issue = Issue::parse(&trailer.item)?;
            found.push(Landed { issue, box_text: trailer.box_text, sha: sha.clone() });
        }
    }
    Ok(found)
}

pub fn queue(queue_directory: &Path, repo: &Path, base: &str, head: &str) -> Vec<String> {
    let landed = match landed(repo, base, head) {
        Ok(landed) => landed,
        Err(error) => return vec![format!("no box ticked: {error}")],
    };
    landed
        .into_iter()
        .map(|landed| {
            let edit = Edit::Land { box_text: landed.box_text.clone(), sha: landed.sha.clone() };
            match issue_body::submit(queue_directory, &landed.issue, &edit) {
                Ok(_) => format!("queued the tick of {} box {:?} landed in {}", landed.issue, landed.box_text, &landed.sha[..landed.sha.len().min(SHORT_SHA)]),
                Err(error) => format!("cannot queue the tick of {} box {:?}: {error}", landed.issue, landed.box_text),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tests;
