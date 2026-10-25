//! Which open issues are ready: unticked Done-when boxes, nobody holding them, nothing for Tom to
//! decide, and every predecessor closed.
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::LazyLock;

pub(crate) const OWNER: &str = "tompassarelli";
/// Labels that hand an issue to Tom: his decision or his judgment is the remaining step.
const TOM_LABELS: [&str; 3] = ["evidence:tom", "needs:tom", "decision:tom"];
/// Labels whose issue is not one box: a roadmap root is finished by its children.
const ROOT_LABELS: [&str; 1] = ["roadmap"];
const RUNNING_LABELS: [&str; 1] = ["native:running"];
const PRIORITIES: [&str; 4] = ["priority:now", "-", "priority:next", "priority:later"];

static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]").expect("box pattern"));
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"github\.com/([\w.-]+)/([\w.-]+)/issues/(\d+)|(?:([\w.-]+)/)?([A-Za-z][\w.-]*)?#(\d+)").expect("reference pattern")
});

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct IssueRef {
    pub repo: String,
    pub number: u64,
}

impl IssueRef {
    pub(crate) fn new(repo: &str, number: u64) -> Self {
        Self { repo: repo.to_lowercase(), number }
    }

    /// `repo#N`, `owner/repo#N` or a GitHub issue URL.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        references(text, "").into_iter().next().filter(|reference| !reference.repo.is_empty())
    }
}

impl fmt::Display for IssueRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Issue {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    /// GitHub's blocked-by relations that are still open.
    pub open_blockers: Vec<IssueRef>,
}

impl Issue {
    pub(crate) fn reference(&self) -> IssueRef {
        IssueRef::new(&self.repo, self.number)
    }

    pub(crate) fn unticked(&self) -> usize {
        UNTICKED.find_iter(&self.body).count()
    }

    pub(crate) fn labelled(&self, names: &[&str]) -> Option<&str> {
        self.labels.iter().map(String::as_str).find(|label| names.contains(label))
    }

    /// The rank `threads unowned` staffs by: priority:now, unprioritized, next, later.
    pub(crate) fn priority(&self) -> usize {
        let label = PRIORITIES.iter().find(|priority| self.labels.iter().any(|label| label == *priority)).unwrap_or(&"-");
        PRIORITIES.iter().position(|priority| priority == label).unwrap_or(1)
    }
}

/// Every issue reference in `text`; a bare `#N` belongs to `repo`.
pub(crate) fn references(text: &str, repo: &str) -> Vec<IssueRef> {
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
pub(crate) fn planned_predecessors(plans: &[Issue]) -> BTreeMap<IssueRef, BTreeSet<IssueRef>> {
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
pub(crate) fn predecessors(issue: &Issue) -> Result<Vec<IssueRef>, String> {
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

pub(crate) struct World<'a> {
    pub open: &'a BTreeSet<IssueRef>,
    pub holds: &'a BTreeMap<IssueRef, String>,
    pub planned: &'a BTreeMap<IssueRef, BTreeSet<IssueRef>>,
}

/// Ok when the issue is ready to staff, else why not.
pub(crate) fn readiness(issue: &Issue, world: &World) -> Result<(), String> {
    let reference = issue.reference();
    if issue.unticked() == 0 {
        return Err("no unticked Done-when box".into());
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
    if let Some(holder) = world.holds.get(&reference) {
        return Err(format!("held by {holder}"));
    }
    let mut waiting = predecessors(issue)?;
    waiting.extend(world.planned.get(&reference).into_iter().flatten().cloned());
    let open: Vec<String> = waiting.iter().filter(|before| world.open.contains(before)).map(ToString::to_string).collect();
    if !open.is_empty() {
        return Err(format!("waits on open {}", open.join(", ")));
    }
    Ok(())
}
