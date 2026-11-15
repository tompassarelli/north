use std::fmt;

use crate::issue_box_lint;

const OBSERVATION_MARKERS: [&str; 2] = ["(observed", "(headless run)"];
const CHECKED: &str = "done when:";
pub const SHORT_SHA: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Ticked(String),
    NoMatch,
    Ambiguous(usize),
    AlreadyTicked,
    NeedsObservation,
    NeedsChecker,
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
            Tick::NeedsChecker => write!(formatter, "landed, box not ticked: its Done-when command did not pass on main, and only that command's passing Evidence stamp ticks it"),
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

/// Whether the box names a backticked command after its `Done when:`, as the unrunnable-box guard reads it (prose may come
/// first), which `north task check --done-when` runs and stamps, so the landing tick leaves it to that check.
fn commanded(text: &str) -> bool {
    let Some(at) = text.to_ascii_lowercase().find(CHECKED) else { return false };
    text[at + CHECKED.len()..].split_once('`').and_then(|(_, rest)| rest.split_once('`')).is_some_and(|(command, _)| !command.is_empty())
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

fn target(lines: &[&str], boxes: &[Checkbox], box_text: &str) -> Result<usize, Tick> {
    if boxes.is_empty() {
        return Err(Tick::NoBoxes);
    }
    let wanted = box_text.trim();
    if wanted.is_empty() {
        return Err(Tick::NoMatch);
    }
    let open: Vec<usize> = boxes.iter().enumerate().filter(|(_, found)| !found.ticked && found.text.contains(wanted)).map(|(index, _)| index).collect();
    let found = match open.as_slice() {
        [found] => *found,
        [] if boxes.iter().any(|found| found.ticked && found.text.contains(wanted)) => return Err(Tick::AlreadyTicked),
        [] => return Err(Tick::NoMatch),
        many => return Err(Tick::Ambiguous(many.len())),
    };
    if needs_observation(boxes[found].text) {
        return Err(Tick::NeedsObservation);
    }
    if commanded(boxes[found].text) {
        return Err(Tick::NeedsChecker);
    }
    let open_children = children(lines, boxes, found).into_iter().filter(|&child| !boxes[child].ticked).count();
    if open_children > 0 {
        return Err(Tick::OpenChildren(open_children));
    }
    Ok(found)
}

fn cascade(lines: &[&str], boxes: &[Checkbox], found: usize) -> Vec<usize> {
    let mut ticked: Vec<bool> = boxes.iter().map(|found| found.ticked).collect();
    ticked[found] = true;
    let mut cascade = vec![found];
    let mut child = found;
    while let Some(up) = parent(lines, boxes, child) {
        if ticked[up] || needs_observation(boxes[up].text) || commanded(boxes[up].text) || !children(lines, boxes, up).iter().all(|&below| ticked[below]) {
            break;
        }
        ticked[up] = true;
        cascade.push(up);
        child = up;
    }
    cascade
}

pub fn tick(body: &str, box_text: &str, sha: Option<&str>) -> Tick {
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let boxes = checkboxes(&lines);
    let found = match target(&lines, &boxes, box_text) {
        Ok(found) => found,
        Err(refused) => return refused,
    };
    let cascade = cascade(&lines, &boxes, found);
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
    !boxes.is_empty() && boxes.iter().all(|found| found.ticked) && north_base::platform::box_oracles::unproven(body).is_empty()
}
