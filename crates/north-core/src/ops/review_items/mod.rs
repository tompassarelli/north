use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::fleet::overlap::{self, Lane};
use crate::ops::citation_check;

static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{2,3} +(.+)$").expect("heading pattern"));
static BOX_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\. +\*\*([A-Z][0-9]+) +([^*]+?)\.?\*\*").expect("box pattern"));
static BOX_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z][0-9]+\b").expect("box id pattern"));
const ENFORCED: &str = "Enforced by";
const SUBJECT_CHARS: usize = 110;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Fact,
    Scenario,
    Architecture,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Scenario => "scenario",
            Self::Architecture => "architecture",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tier {
    SonnetHigh,
    OpusMedium,
    OpusHigh,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::SonnetHigh => "sonnet-high",
            Self::OpusMedium => "opus-medium",
            Self::OpusHigh => "opus-high",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix("claude:").unwrap_or(text);
        [Self::SonnetHigh, Self::OpusMedium, Self::OpusHigh].into_iter().find(|tier| tier.label() == text)
    }

    pub fn scenario_reviewer(self) -> Self {
        match self {
            Self::SonnetHigh => Self::OpusMedium,
            Self::OpusMedium | Self::OpusHigh => Self::SonnetHigh,
        }
    }

    pub fn architecture_reviewer(self) -> Self {
        match self {
            Self::OpusMedium => Self::OpusHigh,
            Self::SonnetHigh | Self::OpusHigh => Self::OpusMedium,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lens {
    Decomposition,
    SimplerAlternative,
}

impl Lens {
    pub fn label(self) -> &'static str {
        match self {
            Self::Decomposition => "decomposition",
            Self::SimplerAlternative => "simpler-alternative",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Handler {
    Holds,
    Fails,
    Agent { tier: Tier, lens: Option<Lens> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub line: usize,
    pub subject: String,
    pub handler: Handler,
}

impl Item {
    fn fact(line: usize, holds: bool, subject: String) -> Self {
        Self { kind: Kind::Fact, line, subject, handler: if holds { Handler::Holds } else { Handler::Fails } }
    }

    pub fn render(&self) -> String {
        let handler = match self.handler {
            Handler::Holds => "holds".to_owned(),
            Handler::Fails => "FAILS".to_owned(),
            Handler::Agent { tier, lens: None } => tier.label().to_owned(),
            Handler::Agent { tier, lens: Some(lens) } => format!("{} lens {}", tier.label(), lens.label()),
        };
        format!("{} {handler} L{}: {}", self.kind.label(), self.line, self.subject)
    }
}

struct Part {
    line: usize,
    title: String,
    body: Vec<(usize, String)>,
}

fn parts(text: &str) -> Vec<Part> {
    let mut found: Vec<Part> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if let Some(heading) = HEADING.captures(line) {
            found.push(Part { line: index + 1, title: heading[1].trim().to_owned(), body: Vec::new() });
        } else if let Some(part) = found.last_mut() {
            part.body.push((index + 1, line.to_owned()));
        }
    }
    found
}

fn blocks(body: &[(usize, String)]) -> Vec<(usize, String)> {
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    for (line, text) in body {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            open = false;
        } else if open && !trimmed.starts_with("- ") {
            if let Some((_, block)) = found.last_mut() {
                block.push(' ');
                block.push_str(trimmed);
            }
        } else {
            found.push((*line, trimmed.trim_start_matches("- ").to_owned()));
            open = true;
        }
    }
    found
}

fn clipped(text: &str) -> String {
    if text.chars().count() <= SUBJECT_CHARS {
        return text.to_owned();
    }
    let cut: String = text.chars().take(SUBJECT_CHARS).collect();
    format!("{}...", cut.trim_end())
}

pub fn design_items(text: &str, author: Tier) -> Vec<Item> {
    let mut found = Vec::new();
    for part in parts(text) {
        let claims: Vec<(usize, String)> = blocks(&part.body)
            .into_iter()
            .filter_map(|(line, block)| block.find(ENFORCED).map(|at| (line, block[at..].to_owned())))
            .collect();
        if claims.is_empty() {
            continue;
        }
        let rejects = part.body.iter().any(|(_, line)| line.contains("Rejected") || line.contains("rejected"));
        let lens = if rejects { Lens::SimplerAlternative } else { Lens::Decomposition };
        found.push(Item {
            kind: Kind::Architecture,
            line: part.line,
            subject: part.title.clone(),
            handler: Handler::Agent { tier: author.architecture_reviewer(), lens: Some(lens) },
        });
        found.extend(claims.into_iter().map(|(line, claim)| Item {
            kind: Kind::Scenario,
            line,
            subject: clipped(&format!("{}: {claim}", part.title)),
            handler: Handler::Agent { tier: author.scenario_reviewer(), lens: None },
        }));
    }
    found
}

struct PlanBox {
    id: String,
    line: usize,
    files: Vec<String>,
    needs: BTreeSet<String>,
}

fn between<'a>(text: &'a str, start: &str, ends: &[&str]) -> &'a str {
    let Some(at) = text.find(start) else { return "" };
    let rest = &text[at + start.len()..];
    let end = ends.iter().filter_map(|end| rest.find(end)).min().unwrap_or(rest.len());
    &rest[..end]
}

fn plan_boxes(text: &str) -> Vec<PlanBox> {
    let mut raw: Vec<(String, usize, String)> = Vec::new();
    let mut open = false;
    for (index, line) in text.lines().enumerate() {
        if let Some(start) = BOX_START.captures(line) {
            raw.push((start[1].to_owned(), index + 1, line.to_owned()));
            open = true;
        } else if open && line.starts_with(' ') && !line.trim().is_empty() {
            if let Some((.., body)) = raw.last_mut() {
                body.push(' ');
                body.push_str(line.trim());
            }
        } else {
            open = false;
        }
    }
    let ids: BTreeSet<String> = raw.iter().map(|(id, ..)| id.clone()).collect();
    raw.into_iter()
        .map(|(id, line, body)| {
            let files = overlap::declared_files(&format!("Files: {}", between(&body, "Files:", &["Needs:", "Done when:"]).trim().trim_end_matches('.')));
            let needs = BOX_ID.find_iter(between(&body, "Needs:", &["Done when:"])).map(|m| m.as_str().to_owned()).filter(|need| ids.contains(need) && *need != id).collect();
            PlanBox { id, line, files, needs }
        })
        .collect()
}

fn ordered(boxes: &[PlanBox]) -> BTreeMap<String, BTreeSet<String>> {
    let direct: BTreeMap<&str, &BTreeSet<String>> = boxes.iter().map(|plan| (plan.id.as_str(), &plan.needs)).collect();
    boxes
        .iter()
        .map(|plan| {
            let mut after: BTreeSet<String> = BTreeSet::new();
            let mut pending: Vec<&String> = plan.needs.iter().collect();
            while let Some(need) = pending.pop() {
                if after.insert(need.clone()) {
                    pending.extend(direct.get(need.as_str()).into_iter().flat_map(|needs| needs.iter()));
                }
            }
            (plan.id.clone(), after)
        })
        .collect()
}

pub fn overlap_facts(text: &str) -> Vec<Item> {
    let boxes = plan_boxes(text);
    let after = ordered(&boxes);
    let mut found = Vec::new();
    for (index, later) in boxes.iter().enumerate() {
        for earlier in &boxes[..index] {
            let lane = Lane { name: earlier.id.clone(), task: Some(earlier.id.clone()), since_ms: earlier.line as u64, files: earlier.files.iter().cloned().collect() };
            for shared in overlap::overlaps(&later.files, &[lane], &|_| false) {
                let order = if after[&later.id].contains(&earlier.id) {
                    Some(format!("{} needs {}", later.id, earlier.id))
                } else {
                    after[&earlier.id].contains(&later.id).then(|| format!("{} needs {}", earlier.id, later.id))
                };
                let files = shared.files.join(", ");
                let subject = match &order {
                    Some(order) => format!("boxes {} and {} both change {files}; {order}", earlier.id, later.id),
                    None => format!("boxes {} and {} both change {files} and no Needs: orders them", earlier.id, later.id),
                };
                found.push(Item::fact(later.line, order.is_some(), subject));
            }
        }
    }
    found
}

pub fn verb_facts(plan: &str, text: &str, defined: fn(&str) -> bool) -> Vec<Item> {
    let mut seen = BTreeSet::new();
    citation_check::scan(plan, text)
        .into_iter()
        .filter(|citation| seen.insert(citation.command.clone()))
        .map(|citation| {
            let holds = defined(&citation.command);
            let verdict = if holds { "defined" } else { "not a defined subcommand" };
            Item::fact(citation.line, holds, format!("verb north {}: {verdict}", citation.command))
        })
        .collect()
}

pub fn path_facts(root: &Path, plan: &str, text: &str) -> Result<Vec<Item>, String> {
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for cited in citation_check::cited_paths(root, plan, text)? {
        if !seen.insert((cited.target.clone(), cited.cited_line)) {
            continue;
        }
        let named = match cited.cited_line {
            Some(line) => format!("{}:{line}", cited.target),
            None => cited.target.clone(),
        };
        let (holds, verdict) = match (&cited.resolved, cited.cited_line) {
            (None, _) => (false, "not tracked".to_owned()),
            (Some(_), None) => (true, "tracked".to_owned()),
            (Some(file), Some(line)) => {
                let lines = std::fs::read_to_string(root.join(file)).map(|text| text.lines().count()).unwrap_or(0);
                (line <= lines, format!("line {line} of {lines}"))
            }
        };
        found.push(Item::fact(cited.line, holds, format!("path {named}: {verdict}")));
    }
    Ok(found)
}

pub fn dispatch(root: &Path, plan: &str, author: Tier, defined: fn(&str) -> bool) -> Result<Vec<Item>, String> {
    let text = std::fs::read_to_string(root.join(plan)).map_err(|error| format!("{plan}: {error}"))?;
    let mut items = path_facts(root, plan, &text)?;
    items.extend(verb_facts(plan, &text, defined));
    items.extend(overlap_facts(&text));
    items.extend(design_items(&text, author));
    items.sort_by_key(|item| (item.kind, item.line));
    Ok(items)
}

pub fn render(plan: &str, author: Tier, items: &[Item]) -> Vec<String> {
    let count = |kind: Kind| items.iter().filter(|item| item.kind == kind).count();
    let failed = items.iter().filter(|item| item.handler == Handler::Fails).count();
    let mut lines = vec![format!(
        "review items {plan}, author {}: {} fact ({failed} fail, checked here), {} scenario -> {}, {} architecture -> {}",
        author.label(),
        count(Kind::Fact),
        count(Kind::Scenario),
        author.scenario_reviewer().label(),
        count(Kind::Architecture),
        author.architecture_reviewer().label(),
    )];
    lines.extend(items.iter().map(Item::render));
    lines
}

const USAGE: &str = "Usage: north review-items PLAN [--author sonnet-high|opus-medium|opus-high] [--root DIR]";

pub fn run(arguments: &[String], defined: fn(&str) -> bool) -> u8 {
    let mut plan = None;
    let mut author = Tier::OpusMedium;
    let mut root = PathBuf::from(".");
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.clone().next()) {
            ("--author", Some(value)) => match Tier::parse(value) {
                Some(tier) => {
                    author = tier;
                    rest.next();
                }
                None => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            },
            ("--root", Some(value)) => {
                root = PathBuf::from(value);
                rest.next();
            }
            (value, _) if plan.is_none() && !value.starts_with("--") => plan = Some(value.to_owned()),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let Some(plan) = plan else {
        eprintln!("{USAGE}");
        return 2;
    };
    match dispatch(&root, &plan, author, defined) {
        Err(error) => {
            eprintln!("review-items: {error}");
            1
        }
        Ok(items) => {
            for line in render(&plan, author, &items) {
                println!("{line}");
            }
            0
        }
    }
}

#[cfg(test)]
mod tests;
