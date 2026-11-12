use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::boxes::{self, Checkbox};
use crate::pacing::placement::needs::declared;
use crate::platform::box_oracles::done_when_command;
use crate::threads::claims;

/// The longest title a created issue may carry.
pub const LONGEST_TITLE: usize = 80;
const SHORTEST_TITLE: usize = 15;
/// GitHub's limit on the sub-issues of one parent.
const MOST_CHILDREN: u64 = 100;
const TYPES: [&str; 3] = ["Task", "Bug", "Feature"];
pub const WAITS: &str = "waits:runtime";
/// The declared needs only the live runtime or a native client serves.
const RUNTIME_NEEDS: [&str; 6] = ["live", "lander", "native", "client", "warcraft", "wc3"];

static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\(?[A-Za-z]{0,2}\d+[a-z]?(?:-\d+)?[.):]\s+").expect("letter code pattern"));
static CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(Verify|Category):\s*([a-z][a-z-]*)").expect("class pattern"));
static OWNERS_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bdecision for (?:tom|the owner)\b").expect("decision pattern"));
static DATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:not before|on or after|after|reviewed on|review on)\s+\d{4}-\d{2}-\d{2}").expect("date pattern"));

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Verdict {
    #[serde(rename = "leaf")]
    Leaf,
    #[serde(rename = "third-level issue")]
    ThirdLevel,
    #[serde(rename = "prose")]
    Prose,
    #[serde(rename = "gate")]
    Gate,
    #[serde(rename = "held by a live claim")]
    Claimed,
}

impl Verdict {
    pub const ALL: [Self; 5] = [Self::Leaf, Self::ThirdLevel, Self::Prose, Self::Gate, Self::Claimed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Leaf => "leaf",
            Self::ThirdLevel => "third-level issue",
            Self::Prose => "prose",
            Self::Gate => "gate",
            Self::Claimed => "held by a live claim",
        }
    }

    fn created(self) -> bool {
        matches!(self, Self::Leaf | Self::ThirdLevel)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Row {
    pub row: usize,
    /// The row of the box this one is indented under.
    pub under: Option<usize>,
    pub verdict: Verdict,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub labels: Vec<String>,
    /// The checkbox line this row stands for, as the body holds it.
    #[serde(rename = "box")]
    pub source: String,
    pub body: String,
    /// Why the machine checks refuse this row; written by the check, never by a reviewer.
    #[serde(default)]
    pub flags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Manifest {
    pub issue: String,
    pub open_boxes: usize,
    pub rows: Vec<Row>,
    /// Why the machine checks refuse the manifest as a whole.
    #[serde(default)]
    pub refused: Vec<String>,
}

/// The issue being converted, as the one read of its repository holds it.
pub struct Parent<'a> {
    /// OWNER/NAME#N.
    pub reference: &'a str,
    pub body: &'a str,
    pub labels: &'a [String],
    pub children: u64,
}

/// The landing's deny-list scan over one row's text: the rules that match it.
pub type Scan = dyn Fn(&str) -> Result<BTreeSet<String>, String>;

/// What the rows are checked against: the repository's open titles and labels, and the landing's deny-list scan.
pub struct Tracker<'a> {
    pub titles: &'a BTreeSet<String>,
    /// None when the repository has more labels than the read returned.
    pub labels: Option<&'a BTreeSet<String>>,
    pub scan: &'a Scan,
}

pub fn title_key(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The box's own first clause when it reads as a title, else nothing: the converter writes no title of its own.
fn own_sentence(first: &str) -> String {
    let text = CODE.replace(first, "");
    let end = [": ", ". ", " (", " ["].iter().filter_map(|stop| text.find(*stop)).min().unwrap_or(text.len());
    let clause = text[..end].trim().trim_end_matches(['.', ':', ',', ';']);
    let length = clause.chars().count();
    let whole = clause.matches('`').count().is_multiple_of(2);
    if (SHORTEST_TITLE..=LONGEST_TITLE).contains(&length) && whole { clause.to_owned() } else { String::new() }
}

fn class(text: &str) -> Option<(&str, &str)> {
    let found = CLASS.captures(text)?;
    Some((found.get(1)?.as_str(), found.get(2)?.as_str()))
}

fn needs(text: &str) -> Vec<String> {
    declared(text).unwrap_or_default()
}

fn owners_call(text: &str) -> bool {
    OWNERS_CALL.is_match(text) || needs(text).iter().any(|need| need == "tom" || need == "owner")
}

fn waits(text: &str) -> bool {
    needs(text).iter().any(|need| RUNTIME_NEEDS.contains(&need.as_str())) || DATED.is_match(text) || class(text).is_some_and(|(key, value)| key == "Verify" && matches!(value, "farm" | "native" | "visual"))
}

fn held(checkbox: &Checkbox, claimed: &[Option<String>]) -> bool {
    let slug = claims::box_slug(&checkbox.first);
    claimed.iter().any(|claim| match (claim, &slug) {
        (None, _) => true,
        (Some(claim), Some(slug)) => claims::same_box(claim, slug),
        (Some(_), None) => false,
    })
}

fn labels(parent: &Parent, text: &str, verdict: Verdict) -> Vec<String> {
    let mut labels: Vec<String> = parent.labels.iter().filter(|label| label.starts_with("area:")).cloned().collect();
    let own = class(text).filter(|(key, _)| *key == "Category" && verdict == Verdict::Leaf).map(|(_, value)| format!("category:{value}"));
    labels.extend(own.or_else(|| parent.labels.iter().find(|label| label.starts_with("category:")).cloned()));
    if verdict == Verdict::Leaf && waits(text) {
        labels.push(WAITS.to_owned());
    }
    labels
}

fn origin(parent: &Parent, checkbox: &Checkbox) -> String {
    format!("Converted from {}, box `{}`.", parent.reference, claims::box_slug(&checkbox.first).unwrap_or_default())
}

struct Outline {
    all: Vec<Checkbox>,
}

impl Outline {
    fn open(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.all.len()).filter(|index| !self.all[*index].ticked)
    }

    /// The unticked checkbox `index` is indented under.
    fn above(&self, index: usize) -> Option<usize> {
        self.all[index].parent.filter(|parent| !self.all[*parent].ticked)
    }
}

fn checked(text: &str) -> bool {
    done_when_command(text).is_some() || class(text).is_some()
}

/// What a box is by its own text, its claim and the box above it, before a reviewer's edit.
fn machine(outline: &Outline, index: usize, claimed: &[Option<String>], above: Option<Verdict>) -> Verdict {
    let checkbox = &outline.all[index];
    let parent = outline.above(index).map(|parent| &outline.all[parent]);
    if above == Some(Verdict::Claimed) || held(checkbox, claimed) {
        return Verdict::Claimed;
    }
    if owners_call(&checkbox.text) {
        return Verdict::Gate;
    }
    if !checked(&checkbox.text) && parent.is_some_and(|parent| done_when_command(&parent.text).is_some()) {
        return Verdict::Prose;
    }
    Verdict::Leaf
}

struct Drafted<'a> {
    parent: &'a Parent<'a>,
    outline: Outline,
    verdicts: BTreeMap<usize, Verdict>,
    edits: BTreeMap<usize, &'a Row>,
}

impl<'a> Drafted<'a> {
    /// Takes the reviewer's verdict for every box their manifest names that no live claim holds.
    fn keep(&mut self, reviewed: &'a [Row]) {
        let mut left: Vec<Option<&Row>> = reviewed.iter().map(Some).collect();
        for (index, verdict) in &mut self.verdicts {
            let named = left.iter_mut().find(|row| row.is_some_and(|row| row.source == self.outline.all[*index].first)).and_then(Option::take);
            if let Some(edit) = named.filter(|edit| *verdict != Verdict::Claimed && edit.verdict != Verdict::Claimed) {
                *verdict = edit.verdict;
                self.edits.insert(*index, edit);
            }
        }
    }

    fn below(&self, index: usize) -> impl Iterator<Item = (usize, Verdict)> + '_ {
        self.verdicts.iter().filter(move |(child, _)| self.outline.above(**child) == Some(index)).map(|(child, verdict)| (*child, *verdict))
    }

    /// A leaf with a box under it that is more than a line of its own landing is an issue with children.
    fn raise(&mut self) {
        let parents: Vec<usize> = self.verdicts.keys().copied().filter(|index| self.below(*index).any(|(_, verdict)| verdict != Verdict::Prose)).collect();
        for index in parents.into_iter().filter(|index| !self.edits.contains_key(index)) {
            if let Some(verdict) = self.verdicts.get_mut(&index).filter(|verdict| **verdict == Verdict::Leaf) {
                *verdict = Verdict::ThirdLevel;
            }
        }
    }

    fn body(&self, index: usize) -> String {
        let checkbox = &self.outline.all[index];
        if !self.verdicts[&index].created() {
            return checkbox.text.clone();
        }
        let mut body = checkbox.text.replacen(" Done when:", "\n\nDone when:", 1).replacen(" Needs:", "\nNeeds:", 1);
        for (line, _) in self.below(index).filter(|(_, verdict)| *verdict == Verdict::Prose) {
            body.push_str(&format!("\n- {}", self.outline.all[line].text.replace('\n', "\n  ")));
        }
        format!("{body}\n\n{}", origin(self.parent, checkbox))
    }

    fn row(&self, index: usize, number: &BTreeMap<usize, usize>) -> Row {
        let (checkbox, verdict) = (&self.outline.all[index], self.verdicts[&index]);
        let mut row = Row { row: number[&index], under: self.outline.above(index).map(|above| number[&above]), verdict, title: String::new(), kind: String::new(), labels: Vec::new(), source: checkbox.first.clone(), body: self.body(index), flags: Vec::new() };
        if verdict.created() {
            row.title = own_sentence(&checkbox.first);
            row.kind = if verdict == Verdict::ThirdLevel { "Feature" } else { "Task" }.to_owned();
            row.labels = labels(self.parent, &checkbox.text, verdict);
        }
        if let Some(edit) = self.edits.get(&index).filter(|_| verdict.created()) {
            row.title.clone_from(&edit.title);
            if !edit.kind.is_empty() {
                row.kind.clone_from(&edit.kind);
            }
            if !edit.labels.is_empty() {
                row.labels.clone_from(&edit.labels);
            }
        }
        row
    }
}

/// One row per unticked checkbox of the parent's body. `claimed` holds the box slug of each live claim on the issue,
/// None for a claim on the whole issue; `reviewed` is the manifest a person or a sorting worker edited, whose verdict,
/// title, type and labels are kept for every box still in the body that no live claim holds.
pub fn draft(parent: &Parent, claimed: &[Option<String>], reviewed: &[Row]) -> Manifest {
    let outline = Outline { all: boxes::checkboxes(parent.body) };
    let mut verdicts: BTreeMap<usize, Verdict> = BTreeMap::new();
    for index in outline.open() {
        let above = outline.above(index).map(|above| verdicts[&above]);
        verdicts.insert(index, machine(&outline, index, claimed, above));
    }
    let mut drafted = Drafted { parent, outline, verdicts, edits: BTreeMap::new() };
    drafted.keep(reviewed);
    drafted.raise();
    let number: BTreeMap<usize, usize> = drafted.verdicts.keys().enumerate().map(|(row, index)| (*index, row + 1)).collect();
    let rows = drafted.verdicts.keys().map(|index| drafted.row(*index, &number)).collect();
    Manifest { issue: parent.reference.to_owned(), open_boxes: boxes::unticked(parent.body), rows, refused: Vec::new() }
}

impl Manifest {
    /// Writes every refusal the machine checks have: on the manifest when its rows do not add up to the body's open
    /// boxes, and on each row that would be created with a title, check, label or word the tracker must not take.
    pub fn check(&mut self, parent: &Parent, tracker: &Tracker) {
        self.open_boxes = boxes::unticked(parent.body);
        self.refused = self.sums(parent);
        let titles: Vec<String> = self.rows.iter().map(|row| title_key(&row.title)).collect();
        for at in 0..self.rows.len() {
            let mut flags = Vec::new();
            if self.rows[at].verdict.created() {
                flags.extend(self.title_flags(at, &titles, tracker));
                flags.extend(content_flags(&self.rows[at], tracker));
            }
            self.rows[at].flags = flags;
        }
    }

    fn sums(&self, parent: &Parent) -> Vec<String> {
        let mut refused = Vec::new();
        if self.rows.len() != self.open_boxes {
            refused.push(format!("{} rows for the {} open boxes of the body", self.rows.len(), self.open_boxes));
        }
        let body = boxes::checkboxes(parent.body);
        let mut seen: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        body.iter().filter(|checkbox| !checkbox.ticked).for_each(|checkbox| seen.entry(checkbox.first.as_str()).or_default().0 += 1);
        self.rows.iter().for_each(|row| seen.entry(row.source.as_str()).or_default().1 += 1);
        refused.extend(seen.iter().filter(|(_, (held, named))| held != named).map(|(text, (held, named))| format!("{named} rows for {held} open boxes that read: {text}")));
        let created = self.rows.iter().filter(|row| row.verdict.created() && row.under.is_none()).count() as u64;
        if parent.children + created > MOST_CHILDREN {
            refused.push(format!("{created} new children beside the {} it has, and GitHub allows {MOST_CHILDREN} under one parent", parent.children));
        }
        refused
    }

    fn title_flags(&self, at: usize, titles: &[String], tracker: &Tracker) -> Vec<String> {
        let title = &self.rows[at].title;
        let length = title.chars().count();
        if title.trim().is_empty() {
            return vec![format!("no title: its own text holds no sentence of {SHORTEST_TITLE} to {LONGEST_TITLE} characters, so a person or a sorting worker writes one")];
        }
        let mut flags = Vec::new();
        if length > LONGEST_TITLE {
            flags.push(format!("its title is {length} characters and the limit is {LONGEST_TITLE}"));
        }
        if CODE.is_match(title) {
            flags.push("its title starts with a letter code".to_owned());
        }
        if tracker.titles.contains(&titles[at]) {
            flags.push("its title equals an open issue's".to_owned());
        }
        if let Some(twin) = (0..at).find(|other| self.rows[*other].verdict.created() && titles[*other] == titles[at]) {
            flags.push(format!("its title equals row {}'s", self.rows[twin].row));
        }
        flags
    }

    pub fn flagged(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|row| !row.flags.is_empty())
    }

    pub fn passes(&self) -> bool {
        self.refused.is_empty() && self.flagged().next().is_none()
    }

    pub fn count_line(&self) -> String {
        let counts: Vec<String> = Verdict::ALL.iter().map(|verdict| format!("{} {}", self.rows.iter().filter(|row| row.verdict == *verdict).count(), verdict.label())).collect();
        format!("{}: {} open boxes, {} rows: {}; {} rows flagged, {} refusals of the manifest", self.issue, self.open_boxes, self.rows.len(), counts.join(", "), self.flagged().count(), self.refused.len())
    }
}

fn content_flags(row: &Row, tracker: &Tracker) -> Vec<String> {
    let mut flags = Vec::new();
    if row.verdict == Verdict::Leaf && !checked(&row.body) {
        flags.push("a leaf with no runnable Done when (a command in backticks) and no declared class (Verify: or Category:)".to_owned());
    }
    if !TYPES.contains(&row.kind.as_str()) {
        flags.push(format!("its type {:?} is not one of {}", row.kind, TYPES.join(", ")));
    }
    if let Some(known) = tracker.labels {
        flags.extend(row.labels.iter().filter(|label| !known.contains(*label)).map(|label| format!("the repository has no label {label}")));
    }
    match (tracker.scan)(&format!("{}\n\n{}", row.title, row.body)) {
        Ok(rules) if rules.is_empty() => {}
        Ok(rules) => flags.push(format!("the privacy scan refuses it (deny-list rule {})", rules.into_iter().collect::<Vec<_>>().join(", "))),
        Err(why) => flags.push(format!("the privacy scan could not read it, so it is refused: {why}")),
    }
    flags
}
