//! Which open issues are ready: unticked Done-when boxes, nobody holding them, no label or open gate handing them to
//! Tom, no `(not before ...)` stamp ahead, and every predecessor closed. A box waits only on records: its stamp, the fact
//! graph's box edges, a gate, a claim or a registered wait; its prose and the newest report hold nothing (north#872).
use crate::platform::box_oracles;
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

static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]").expect("box pattern"));
static ANY_BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[[ xX]\]").expect("any box pattern"));
static MISSED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<!--\s*check\s[^>]*\bmissed\b[^>]*-->").expect("missed pattern"));
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
    /// The landings on origin/main citing it since its last report, read by the survey for an issue with a box due after landings.
    pub landings: usize,
    /// The issue it is a sub-issue of, when GitHub holds one.
    pub parent: Option<IssueRef>,
    /// GitHub's issue type (Task, Bug, Feature), when set.
    pub kind: Option<String>,
    pub assignees: Vec<String>,
    /// How many sub-issues it has and how many of them are closed.
    pub children: Children,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Children {
    pub total: u64,
    pub completed: u64,
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
        !self.body.lines().any(|line| ANY_BOX.is_match(line)) && [&TOM_LABELS[..], &ROOT_LABELS, &RUNNING_LABELS, &crate::platform::issue_scope::WORKFLOW_OWNED].iter().all(|names| self.labelled(names).is_none())
    }

    /// The live sessions that own every unticked box, or none when some unticked box names no live owner.
    pub fn owned_by(&self, live: &BTreeSet<String>) -> Option<BTreeSet<String>> {
        let mut owners = BTreeSet::new();
        for line in self.body.lines().filter(|line| UNTICKED.is_match(line)) {
            let named: Vec<String> = box_owners(line).into_iter().filter(|owner| live.contains(owner)).collect();
            if named.is_empty() {
                return None;
            }
            owners.extend(named);
        }
        (!owners.is_empty()).then_some(owners)
    }

    /// The unticked product boxes whose stamped check, if any, has come by `at`: the boxes a session launched now can take.
    pub fn takeable(&self, at: f64) -> impl Iterator<Item = &str> {
        self.product_holds().filter(move |(line, hold)| hold.is_none() && !self.awaits_check(line, at)).map(|(line, _)| line)
    }

    /// Each unticked product box with what holds it beyond a date: a live reading its command takes, or its own `Needs: Tom` line.
    fn product_holds(&self) -> impl Iterator<Item = (&str, Option<Hold>)> {
        self.product_lines().map(|line| {
            let hold = live_reading(line).map(Hold::Live).or_else(|| crate::pacing::placement::needs::declared(line).is_some_and(|needs| needs.iter().any(|need| need == "tom")).then_some(Hold::Tom));
            (line, hold)
        })
    }

    /// Whether the box `line` still waits for its check: a pending check the experiments judge runs, a date ahead of `at`, or more landings than have cited the issue since its last report.
    fn awaits_check(&self, line: &str, at: f64) -> bool {
        box_oracles::deferred_to(line).is_some() || north_graph_ground::due_pending(line, at as i64).is_some() || north_graph_ground::due_landings(line).is_some_and(|wanted| self.landings < wanted)
    }

    /// Whether some unticked product box is due after a count of landings, which the survey counts from the checkout.
    pub fn counts_landings(&self) -> bool {
        self.product_lines().any(|line| north_graph_ground::due_landings(line).is_some())
    }

    /// Whether a missed check reopened the box a launch staffs: its own box when the brief names one, else
    /// any unticked box of the issue.
    pub fn reopened(&self, box_text: Option<&str>) -> bool {
        let mut unticked = self.body.lines().filter(|line| UNTICKED.is_match(line));
        match box_text.map(str::trim).filter(|text| !text.is_empty()) {
            Some(text) => unticked.filter(|line| line.contains(text)).any(|line| MISSED.is_match(line)),
            None => unticked.any(|line| MISSED.is_match(line)),
        }
    }

    /// The takeable boxes whose text holds `text`, the Done-when box a lane's `Box:` line names.
    pub fn takeable_box<'a>(&'a self, text: &'a str, at: f64) -> impl Iterator<Item = &'a str> {
        let wanted = text.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("[ ]").trim();
        self.takeable(at).filter(move |line| !wanted.is_empty() && line.contains(wanted))
    }

    /// How many workers may take this issue's boxes at once, its `Workers: N` line; without one, the free slots are the only limit.
    pub fn worker_cap(&self) -> Option<usize> {
        super::facets::declared(self, "Workers").and_then(|count| count.split_whitespace().next()?.parse().ok()).filter(|count| *count > 0)
    }

    /// The takeable boxes no worker in `held` has, as the text of each line without the checkbox; a box line written twice is one box, by slug.
    pub fn free_boxes<'a>(&'a self, held: &'a BTreeSet<String>, at: f64) -> impl Iterator<Item = &'a str> {
        let mut seen = BTreeSet::new();
        self.takeable(at).map(|line| line.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("[ ]").trim()).filter(move |text| crate::threads::claims::box_slug(text).is_some_and(|slug| !held.contains(&slug) && seen.insert(slug)))
    }

    /// The needs the boxes `lines` declare with their own `Needs:` lines, and the lander host a check reads its records from;
    /// a box with no `Needs:` line declares none of its prose, whatever it mentions.
    pub fn needs_in<'a>(&self, lines: impl Iterator<Item = &'a str>) -> Vec<String> {
        let mut found: Vec<String> = lines
            .flat_map(|line| {
                let mut needs = crate::pacing::placement::needs::declared(line).unwrap_or_default();
                needs.extend(crate::pacing::placement::needs::implied(line));
                needs
            })
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// What the first box a live reading or Tom holds waits for, else the earliest dated check among the unticked product boxes, else the fewest landings still missing, when every one of them is still waiting.
    pub fn gated_until(&self, at: f64) -> Option<String> {
        if self.takeable(at).next().is_some() {
            return None;
        }
        let dated = self.product_lines().filter_map(north_graph_ground::due).min().map(|(_, written)| written);
        let counted = || {
            let missing = self.product_lines().filter_map(north_graph_ground::due_landings).min()?.saturating_sub(self.landings);
            Some(format!("{missing} more landings cite it"))
        };
        let deferred = || self.product_lines().find_map(box_oracles::deferred_to).map(|id| format!("the judge runs dated check {id}"));
        self.product_holds().find_map(|(_, hold)| hold).map(|hold| hold.to_string()).or(dated).or_else(deferred).or_else(counted)
    }

    /// The first of `held` whose box slug names no box of this issue, ticked or not: a claim that points at no box holds the whole issue.
    pub fn unmatched_hold<'a>(&self, held: &'a BTreeMap<String, String>) -> Option<(&'a str, &'a str)> {
        let slugs: BTreeSet<String> = self.body.lines().filter(|line| ANY_BOX.is_match(line)).filter_map(|line| crate::threads::claims::box_slug(line.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("[x]").trim_start_matches("[X]"))).collect();
        held.iter().find(|(slug, _)| !slugs.contains(*slug)).map(|(slug, why)| (slug.as_str(), why.as_str()))
    }

    fn product_lines(&self) -> impl Iterator<Item = &str> {
        self.body.lines().filter(|line| UNTICKED.is_match(line))
    }

    pub fn labelled(&self, names: &[&str]) -> Option<&str> {
        self.labels.iter().map(String::as_str).find(|label| names.contains(label))
    }

    /// The rank `threads unowned` staffs by: priority:now, unprioritized, next, later; within a priority, docs/rationale/backlog-ranking.md's order.
    pub fn priority(&self) -> usize {
        north_graph_ground::priority(self.labels.iter().map(String::as_str))
    }
}

/// What holds an unticked box when that is not its date.
enum Hold {
    Live(&'static str),
    Tom,
}

impl fmt::Display for Hold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tom => write!(f, "Tom answers it: its own `Needs: Tom` line hands it to him"),
            Self::Live(source) => write!(f, "it reads {source}, which a worker cannot take"),
        }
    }
}

fn live_reading(line: &str) -> Option<&'static str> {
    let oracle = line.find("Done when").map_or(line, |at| &line[at..]);
    let oracle = oracle.to_lowercase();
    let names = |words: &[&str]| words.iter().any(|word| oracle.contains(word));
    if names(&["journalctl", "lander's journal", "lander journal", "landers journal"]) {
        return Some("a live journal");
    }
    if names(&["on the lander host"]) {
        return Some("a host's live ledger");
    }
    if names(&["north dash", "north ledger scoreboard", "north land reuse report"]) {
        return Some("a live ledger report");
    }
    None
}

/// Whether `body` holds the box `text` names, as the `Box:` line of a brief does, and every line holding it is ticked.
pub fn box_ticked(body: &str, text: &str) -> bool {
    let wanted = text.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("[ ]").trim();
    let mut lines = body.lines().filter(|line| !wanted.is_empty() && ANY_BOX.is_match(line) && line.contains(wanted)).peekable();
    lines.peek().is_some() && lines.all(|line| !UNTICKED.is_match(line))
}

/// The unticked Done-when boxes in `body`, the clean-up group included.
pub fn unticked_boxes(body: &str) -> usize {
    UNTICKED.find_iter(body).count()
}

/// The unticked Done-when boxes in `body`, not counting the clean-up group every product issue carries.
pub fn product_boxes(body: &str) -> usize {
    body.lines().filter(|line| UNTICKED.is_match(line)).count()
}

/// Why a worker launch on `item` cannot proceed when its body holds no open Done-when box: a worker works a box.
pub(crate) fn no_box_refusal(item: &str, body: &str) -> Option<String> {
    (unticked_boxes(body) == 0).then(|| format!("{item} has no open Done-when box, so no worker can take it; add the box the work delivers, or launch a brief that names no issue"))
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
    /// The boxes workers hold, by box slug.
    pub box_holds: &'a super::record::BoxHolds,
    /// The boxes dispatch tried twice in a day without ticking, by box slug, which wait for the proxy and count toward no cap.
    pub box_tries: &'a super::record::BoxHolds,
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
    if let Some(label) = issue.labelled(&TOM_LABELS) {
        return Err(format!("labelled {label}, so Tom decides"));
    }
    if let Some(label) = issue.labelled(&ROOT_LABELS) {
        return Err(format!("labelled {label}: its child issues are the boxes"));
    }
    if let Some(label) = issue.labelled(&RUNNING_LABELS) {
        return Err(format!("held: labelled {label}"));
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
    let held = world.box_holds.get(&reference);
    if let Some((slug, holder)) = held.and_then(|boxes| issue.unmatched_hold(boxes)) {
        return Err(format!("held by {holder}: its claim on box `{slug}` names no box of this issue, so it holds them all"));
    }
    if let (Some(cap), Some(boxes)) = (issue.worker_cap(), held)
        && boxes.len() >= cap
    {
        return Err(format!("{} workers hold its boxes, which is its cap of {cap}", boxes.len()));
    }
    let tried = world.box_tries.get(&reference);
    let taken: BTreeSet<String> = held.into_iter().chain(tried).flat_map(|boxes| boxes.keys().cloned()).collect();
    if issue.takeable(world.at).next().is_some() && issue.free_boxes(&taken, world.at).next().is_none() {
        return Err(match tried.and_then(|boxes| boxes.values().next()) {
            Some(why) => format!("no box is free: each has a worker or was tried twice in 24 h: {why}"),
            None => "no box is free: each has a worker".into(),
        });
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
