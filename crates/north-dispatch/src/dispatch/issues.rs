//! Which open issues are ready: unticked Done-when boxes, nobody holding them, no label or newest
//! report handing them to Tom or the laptop, and every predecessor closed.
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::LazyLock;

pub const OWNER: &str = "tompassarelli";
/// Labels that hand an issue to Tom: his decision or his judgment is the remaining step.
const TOM_LABELS: [&str; 3] = ["evidence:tom", "needs:tom", "decision:tom"];
/// Labels whose issue is not one box: a roadmap root is finished by its children.
const ROOT_LABELS: [&str; 1] = ["roadmap"];
const RUNNING_LABELS: [&str; 1] = ["native:running"];
const PRIORITIES: [&str; 4] = ["priority:now", "-", "priority:next", "priority:later"];
/// Why dispatch skips an open issue with no Done-when box at all, which the triage lead gives one.
pub const NEEDS_BOX: &str = "needs a box: no Done-when box";

static WAITS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:(?:waits?|waiting|blocked|held)\s+(?:on|for|by)|awaiting|needs)\s+(?:Tom\b|the laptop\b)|\bnext step is Tom's\b|\bStop:\s*decision\b").expect("waits pattern")
});
static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]").expect("box pattern"));
static ANY_BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[[ xX]\]").expect("any box pattern"));
static NOT_BEFORE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(not before ((\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})Z)\)").expect("not-before pattern"));
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"github\.com/([\w.-]+)/([\w.-]+)/issues/(\d+)|(?:([\w.-]+)/)?([A-Za-z][\w.-]*)?#(\d+)").expect("reference pattern")
});

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct IssueRef {
    pub repo: String,
    pub number: u64,
}

impl IssueRef {
    pub fn new(repo: &str, number: u64) -> Self {
        Self { repo: repo.to_lowercase(), number }
    }

    /// `repo#N`, `owner/repo#N` or a GitHub issue URL.
    pub fn parse(text: &str) -> Option<Self> {
        references(text, "").into_iter().next().filter(|reference| !reference.repo.is_empty())
    }
}

impl fmt::Display for IssueRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Issue {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    /// GitHub's blocked-by relations that are still open.
    pub open_blockers: Vec<IssueRef>,
    /// The newest comment, where a worker reports what the box waits on.
    pub report: String,
    /// The newest comments, oldest first, which `Blocked:` reports are read from.
    pub comments: Vec<serde_json::Value>,
    /// The newest of its body edit and last comment, as unix seconds.
    pub touched: f64,
    /// When it was opened, as unix seconds; dispatch staffs the oldest ready issue first.
    pub created: f64,
    /// When the last dispatched worker's final report named `Stop: decision` or opened `Blocked:`.
    pub stopped: Option<f64>,
}

impl Issue {
    pub fn reference(&self) -> IssueRef {
        IssueRef::new(&self.repo, self.number)
    }

    pub fn unticked(&self) -> usize {
        UNTICKED.find_iter(&self.body).count()
    }

    /// An issue with no box, ticked or not, that no label hands to Tom, its children or a running native job.
    pub fn needs_box(&self) -> bool {
        !ANY_BOX.is_match(&self.body) && [&TOM_LABELS[..], &ROOT_LABELS, &RUNNING_LABELS].iter().all(|names| self.labelled(names).is_none())
    }

    pub fn gated_until(&self, at: f64) -> Option<&str> {
        let mut earliest: Option<(i64, &str)> = None;
        for line in self.body.lines().filter(|line| UNTICKED.is_match(line)) {
            let (when, text) = NOT_BEFORE.captures_iter(line).filter_map(|found| Some((not_before(&found)?, found.get(1)?.as_str()))).max()?;
            if when as f64 <= at {
                return None;
            }
            earliest = Some(earliest.map_or((when, text), |first| first.min((when, text))));
        }
        earliest.map(|(_, text)| text)
    }

    pub fn labelled(&self, names: &[&str]) -> Option<&str> {
        self.labels.iter().map(String::as_str).find(|label| names.contains(label))
    }

    /// The sentence of the newest report that hands the box to Tom or the laptop, if one does.
    pub fn waiting(&self) -> Option<&str> {
        let negated = |start: usize| ["no longer ", "not ", "n't "].iter().any(|word| self.report[..start].to_lowercase().ends_with(word));
        let found = WAITS.find_iter(&self.report).find(|found| !negated(found.start()))?;
        let start = self.report[..found.start()].rfind(['.', '\n']).map_or(0, |end| end + 1);
        let end = self.report[found.end()..].find(['.', '\n']).map_or(self.report.len(), |end| found.end() + end);
        Some(self.report[start..end].trim())
    }

    /// The rank `threads unowned` staffs by: priority:now, unprioritized, next, later.
    pub fn priority(&self) -> usize {
        let label = PRIORITIES.iter().find(|priority| self.labels.iter().any(|label| label == *priority)).unwrap_or(&"-");
        PRIORITIES.iter().position(|priority| priority == label).unwrap_or(1)
    }
}

/// Every issue reference in `text`; a bare `#N` belongs to `repo`.
pub fn references(text: &str, repo: &str) -> Vec<IssueRef> {
    REFERENCE
        .captures_iter(text)
        .filter(|found| {
            let start = found.get(0).map_or(0, |m| m.start());
            !text[..start].ends_with(|c: char| c.is_alphanumeric() || c == '&')
        })
        .filter_map(|found| {
            let group = |index| found.get(index).map(|m| m.as_str());
            match (group(1), group(3)) {
                (Some(owner), Some(number)) if owner.eq_ignore_ascii_case(OWNER) => Some(IssueRef::new(group(2)?, number.parse().ok()?)),
                (Some(_), _) => None,
                _ => {
                    let owner = group(4);
                    if owner.is_some_and(|owner| !owner.eq_ignore_ascii_case(OWNER)) {
                        return None;
                    }
                    Some(IssueRef::new(group(5).unwrap_or(repo), group(6)?.parse().ok()?))
                }
            }
        })
        .collect()
}

fn not_before(found: &regex::Captures) -> Option<i64> {
    let part = |index: usize| found.get(index)?.as_str().parse::<u8>().ok();
    let year = found.get(2)?.as_str().parse().ok()?;
    let date = time::Date::from_calendar_date(year, time::Month::try_from(part(3)?).ok()?, part(4)?).ok()?;
    Some(date.with_time(time::Time::from_hms(part(5)?, part(6)?, 0).ok()?).assume_utc().unix_timestamp())
}

fn line_values<'a>(body: &'a str, key: &str) -> impl Iterator<Item = &'a str> {
    body.lines().filter_map(move |line| {
        let line = line.trim().trim_start_matches(['-', '*', '>', ' ']);
        let (name, value) = line.split_once(':')?;
        name.trim().trim_matches('*').eq_ignore_ascii_case(key).then_some(value)
    })
}

/// The refs each plan's "Order of work" section lists, in order.
fn orders_of_work(issue: &Issue) -> Vec<IssueRef> {
    let mut listed = Vec::new();
    let mut inside = false;
    for line in issue.body.lines() {
        let heading = line.trim_start().starts_with("# ") || line.trim_start().starts_with("##");
        if line.to_lowercase().contains("order of work") {
            inside = true;
            continue;
        }
        if inside && heading {
            inside = false;
        }
        if inside {
            listed.extend(references(line, &issue.repo));
        }
    }
    listed
}

/// Predecessors named by the plan orders of `plans`: everything listed before the issue.
pub fn planned_predecessors(plans: &[Issue]) -> BTreeMap<IssueRef, BTreeSet<IssueRef>> {
    let mut before: BTreeMap<IssueRef, BTreeSet<IssueRef>> = BTreeMap::new();
    for plan in plans {
        let order = orders_of_work(plan);
        for (index, item) in order.iter().enumerate() {
            before.entry(item.clone()).or_default().extend(order[..index].iter().filter(|earlier| *earlier != item).cloned());
        }
    }
    before
}

/// The issues this one waits on: `Follows:` and `Blocked by` refs and GitHub blocked-by relations;
/// Err when a `Follows:` line names no issue, which follows-release starts instead.
pub fn predecessors(issue: &Issue) -> Result<Vec<IssueRef>, String> {
    let mut found = issue.open_blockers.clone();
    for follows in line_values(&issue.body, "Follows") {
        let named = references(follows, &issue.repo);
        if named.is_empty() {
            return Err(format!("Follows: {} names no issue, so follows-release starts it", follows.trim()));
        }
        found.extend(named);
    }
    for line in issue.body.lines() {
        if let Some(at) = line.to_lowercase().find("blocked by") {
            found.extend(references(&line[at..], &issue.repo));
        }
    }
    found.retain(|reference| *reference != issue.reference());
    found.sort();
    found.dedup();
    Ok(found)
}

pub struct World<'a> {
    pub open: &'a BTreeSet<IssueRef>,
    pub holds: &'a BTreeMap<IssueRef, String>,
    pub planned: &'a BTreeMap<IssueRef, BTreeSet<IssueRef>>,
    pub at: f64,
}

/// Ok when the issue is ready to staff, else why not.
pub fn readiness(issue: &Issue, world: &World) -> Result<(), String> {
    let reference = issue.reference();
    if issue.unticked() == 0 {
        return Err(if issue.needs_box() { NEEDS_BOX } else { "no unticked Done-when box" }.into());
    }
    if let Some(until) = issue.gated_until(world.at) {
        let boxes = if issue.unticked() == 1 { "only open box waits" } else { "every open box waits, the first" };
        return Err(format!("{boxes} until {until}"));
    }
    if let Some(label) = issue.labelled(&TOM_LABELS) {
        return Err(format!("labelled {label}, so Tom decides"));
    }
    if let Some(label) = issue.labelled(&ROOT_LABELS) {
        return Err(format!("labelled {label}: its child issues are the boxes"));
    }
    if let Some(label) = issue.labelled(&RUNNING_LABELS) {
        return Err(format!("held: labelled {label}"));
    }
    if let Some(sentence) = issue.waiting() {
        return Err(format!("its newest report waits: {sentence}"));
    }
    let labels: Vec<&str> = issue.labels.iter().map(String::as_str).collect();
    if let Some(reason) = crate::threads::withheld::withheld(&labels, &issue.comments, world.at) {
        return Err(format!("a worker reported it blocked: {reason}"));
    }
    let mut waiting = predecessors(issue)?;
    waiting.extend(world.planned.get(&reference).into_iter().flatten().cloned());
    let open: Vec<String> = waiting.iter().filter(|before| world.open.contains(before)).map(ToString::to_string).collect();
    if !open.is_empty() {
        return Err(format!("waits on open {}", open.join(", ")));
    }
    if let Some(holder) = world.holds.get(&reference) {
        return Err(format!("held by {holder}"));
    }
    if super::stops::awaits_decision(issue) {
        return Err("last worker stopped for a decision".into());
    }
    Ok(())
}
