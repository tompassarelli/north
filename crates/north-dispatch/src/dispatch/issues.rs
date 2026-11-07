//! Which open issues are ready: unticked Done-when boxes, nobody holding them, no label or newest
//! report handing them to Tom or the laptop, and every predecessor closed.
use crate::platform::cleanup;
use north_graph_ground::{Backlog, Verdict};
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
/// Why dispatch skips an open issue with no Done-when box at all, which the triage lead gives one.
pub const NEEDS_BOX: &str = "needs a box: no Done-when box";

static WAITS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:(?:waits?|waiting|blocked|held)\s+(?:on|for|by)|awaiting|needs)\s+(?:Tom\b|the laptop\b)|\bnext step is Tom's\b|\bStop:\s*decision\b").expect("waits pattern")
});
static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]").expect("box pattern"));
static ANY_BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[[ xX]\]").expect("any box pattern"));
static WINDOW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\d+)\s*(min|minutes?|h|hours?|d|days?)\s+(?:cells?|windows?)\b|\b(?:over|after|within)\s+(?:the\s+(?:next|last)\s+)?(\d+)\s*(h|hours?|d|days?)\b").expect("window pattern")
});
static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{9,12}\b").expect("sha pattern"));
static OWNERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\(owners?:\s*([^)]*)\)|\bowner:\s*([A-Za-z0-9][\w.:-]*)").expect("owner pattern"));
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
    /// What the last dispatched worker's final report holds the box for.
    pub stop: super::stops::Stop,
    /// What its takeable boxes name they need beyond its own repository, read from their text by the survey.
    pub needs: Vec<String>,
    /// The newest landing the body cites, as unix seconds, read by the survey for an issue with a windowed box; 0 when none.
    pub landed: f64,
    /// The latest `Stop: wait until` a launch on one of its takeable boxes learned (`wait_stops`), as written and as unix seconds.
    pub learned_wait: Option<(String, f64)>,
    /// The landings on origin/main citing it since its last report, read by the survey for an issue with a box due after landings.
    pub landings: usize,
}

impl Issue {
    pub fn reference(&self) -> IssueRef {
        IssueRef::new(&self.repo, self.number)
    }

    pub fn unticked(&self) -> usize {
        unticked_boxes(&self.body)
    }

    pub fn product_unticked(&self) -> usize {
        product_boxes(&self.body)
    }

    /// An issue with no box, ticked or not, that no label hands to Tom, its children, a running native job or the workflow that owns it.
    pub fn needs_box(&self) -> bool {
        !self.body.lines().any(|line| ANY_BOX.is_match(line) && !cleanup::is_box(line)) && [&TOM_LABELS[..], &ROOT_LABELS, &RUNNING_LABELS, &cleanup::WORKFLOW_OWNED].iter().all(|names| self.labelled(names).is_none())
    }

    /// The live sessions that own every unticked box, or none when some unticked box names no live owner.
    pub fn owned_by(&self, live: &BTreeSet<String>) -> Option<BTreeSet<String>> {
        let mut owners = BTreeSet::new();
        for line in self.body.lines().filter(|line| UNTICKED.is_match(line) && !cleanup::is_box(line)) {
            let named: Vec<String> = box_owners(line).into_iter().filter(|owner| live.contains(owner)).collect();
            if named.is_empty() {
                return None;
            }
            owners.extend(named);
        }
        (!owners.is_empty()).then_some(owners)
    }

    /// The unticked product boxes whose dated check, if any, has come by `at`: the boxes a session launched now can take.
    pub fn takeable(&self, at: f64) -> impl Iterator<Item = &str> {
        self.product_lines().filter(move |line| !self.awaits_check(line, at))
    }

    /// Whether the box `line` still waits for its check: a date ahead of `at`, or more landings than have cited the issue since its last report.
    fn awaits_check(&self, line: &str, at: f64) -> bool {
        north_graph_ground::due_pending(line, at as i64).is_some() || north_graph_ground::due_landings(line).is_some_and(|wanted| self.landings < wanted)
    }

    /// Whether some unticked product box is due after a count of landings, which the survey counts from the checkout.
    pub fn counts_landings(&self) -> bool {
        self.product_lines().any(|line| north_graph_ground::due_landings(line).is_some())
    }

    /// The needs its takeable boxes name (`needs::in_box`): the laptop, a host of `hosts`, or another repository's checkout.
    pub fn box_needs(&self, hosts: &[String], at: f64) -> Vec<String> {
        self.needs_in(self.takeable(at), hosts)
    }

    /// The takeable boxes whose text holds `text`, the Done-when box a lane's `Box:` line names.
    pub fn takeable_box<'a>(&'a self, text: &'a str, at: f64) -> impl Iterator<Item = &'a str> {
        let wanted = text.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("[ ]").trim();
        self.takeable(at).filter(move |line| !wanted.is_empty() && line.contains(wanted))
    }

    pub fn needs_in<'a>(&self, lines: impl Iterator<Item = &'a str>, hosts: &[String]) -> Vec<String> {
        let repos = &crate::threads::catalog::CATALOG.repos;
        let mut found: Vec<String> = lines.flat_map(|line| crate::pacing::placement::needs::in_box(line, &self.repo, hosts, repos)).collect();
        found.sort();
        found.dedup();
        found
    }

    /// The earliest dated check among the unticked product boxes, else the fewest landings still missing, when every one of them is still waiting.
    pub fn gated_until(&self, at: f64) -> Option<String> {
        if self.takeable(at).next().is_some() {
            return None;
        }
        let dated = self.product_lines().filter_map(north_graph_ground::due).min().map(|(_, written)| written);
        dated.or_else(|| {
            let missing = self.product_lines().filter_map(north_graph_ground::due_landings).min()?.saturating_sub(self.landings);
            Some(format!("{missing} more landings cite it"))
        })
    }

    /// The commits the body cites, which the survey dates to anchor a windowed box.
    pub fn cited_shas(&self) -> Vec<&str> {
        SHA.find_iter(&self.body).map(|found| found.as_str()).filter(|sha| sha.bytes().any(|byte| byte.is_ascii_digit())).collect()
    }

    /// Whether some unticked product box names a window over readings (`1 h cell`, `over 6 h`, `after 24 h`).
    pub fn windowed(&self) -> bool {
        self.product_lines().any(|line| window_seconds(line).is_some())
    }

    /// When the earliest windowed box can be read, while every takeable box is windowed and that time is ahead of `at`:
    /// its window after the newest landing the issue cites, else after its last report.
    pub fn window_until(&self, at: f64) -> Option<f64> {
        let anchor = if self.landed > 0.0 { self.landed } else { self.touched };
        let mut until = None::<f64>;
        for line in self.takeable(at) {
            let ready = anchor + window_seconds(line)?;
            until = Some(until.map_or(ready, |earliest| earliest.min(ready)));
        }
        until.filter(|ready| *ready > at)
    }

    fn product_lines(&self) -> impl Iterator<Item = &str> {
        self.body.lines().filter(|line| UNTICKED.is_match(line) && !cleanup::is_box(line))
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

    /// The rank `threads unowned` staffs by: priority:now, unprioritized, next, later; within a priority, docs/rationale/backlog-ranking.md's order.
    pub fn priority(&self) -> usize {
        north_graph_ground::priority(self.labels.iter().map(String::as_str))
    }
}

/// The longest window over readings a box line names, in seconds.
fn window_seconds(line: &str) -> Option<f64> {
    WINDOW
        .captures_iter(line)
        .filter_map(|found| {
            let (count, unit) = if found.get(1).is_some() { (found.get(1)?, found.get(2)?) } else { (found.get(3)?, found.get(4)?) };
            let per = match unit.as_str().to_lowercase().chars().next()? {
                'm' => 60.0,
                'h' => 3600.0,
                _ => 86400.0,
            };
            Some(count.as_str().parse::<f64>().ok()? * per)
        })
        .fold(None, |longest, seconds| Some(longest.map_or(seconds, |before: f64| before.max(seconds))))
}

/// The unticked Done-when boxes in `body`, the clean-up group included.
pub fn unticked_boxes(body: &str) -> usize {
    UNTICKED.find_iter(body).count()
}

/// The unticked Done-when boxes in `body`, not counting the clean-up group every product issue carries.
pub fn product_boxes(body: &str) -> usize {
    body.lines().filter(|line| UNTICKED.is_match(line) && !cleanup::is_box(line)).count()
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
    /// The fleet sessions every reached registry lists as running or parked on a wait.
    pub live: &'a BTreeSet<String>,
    pub planned: &'a BTreeMap<IssueRef, BTreeSet<IssueRef>>,
    /// The fact graph's box waits and ranks, when it could be read.
    pub backlog: Option<&'a Backlog>,
    pub at: f64,
}

/// Ok when the issue is ready to staff, else why not.
pub fn readiness(issue: &Issue, world: &World) -> Result<(), String> {
    let reference = issue.reference();
    if issue.unticked() == 0 {
        return Err(if issue.needs_box() { NEEDS_BOX } else { "no unticked Done-when box" }.into());
    }
    if super::facets::category(issue).is_none() {
        return Err(super::facets::UNCATEGORISED.into());
    }
    if let Some(adds) = super::facets::overbuilt(issue) {
        return Err(format!("a planning or design box adds {adds} with no `Simplest:` line naming the smallest change that meets its Done when and why it falls short"));
    }
    if let Some(owners) = issue.owned_by(world.live) {
        let owners: Vec<&str> = owners.iter().map(String::as_str).collect();
        return Err(format!("every open box has a live owner: {}", owners.join(", ")));
    }
    if let Some(until) = issue.gated_until(world.at) {
        let boxes = if issue.product_unticked() == 1 { "only open box waits" } else { "every open box waits, the first" };
        return Err(format!("{boxes} until {until}"));
    }
    if let Some(ready) = issue.window_until(world.at) {
        let written = crate::foundation::clock::Timestamp::from_unix_seconds_f64(ready).iso_minute();
        return Err(format!("no open box can be read before {written}: each names a window over readings, counted from the newest landing it cites or its last report"));
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
    if let Some(until) = super::stops::waits_until(issue, world.at) {
        return Err(format!("its last worker stopped to wait until {until}"));
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
    if let Some(Verdict::Held(edge)) = world.backlog.map(|backlog| backlog.verdict(&reference.to_string())) {
        return Err(format!("no box is ready: {edge}"));
    }
    if let Some(holder) = world.holds.get(&reference) {
        return Err(format!("held by {holder}"));
    }
    if super::stops::awaits_decision(issue) {
        return Err("last worker stopped for a decision".into());
    }
    Ok(())
}

/// The sessions a box line names as its owners: `(owner: NAME)`, `(owners: n-cull-it, -landing)`, where a leading
/// `-` keeps the previous name's stem, or `Owner: NAME`.
fn box_owners(line: &str) -> Vec<String> {
    let mut owners: Vec<String> = Vec::new();
    for found in OWNERS.captures_iter(line) {
        let listed = found.get(1).or_else(|| found.get(2)).map_or("", |named| named.as_str());
        for word in listed.split([',', ';']).filter_map(|part| part.split_whitespace().next()).map(|word| word.trim_end_matches('.')) {
            let name = match (word.strip_prefix('-'), owners.last()) {
                (Some(rest), Some(before)) => format!("{}-{rest}", before.rsplit_once('-').map_or(before.as_str(), |(stem, _)| stem)),
                _ => word.to_owned(),
            };
            if !name.is_empty() {
                owners.push(name);
            }
        }
    }
    owners
}
